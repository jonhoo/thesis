use cliff::ExponentialCliffSearcher;
use color_eyre::Report;
use eyre::WrapErr;
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    stream::StreamExt,
};
use tracing::instrument;
use tracing_futures::Instrument;
use tsunami::providers::aws;
use tsunami::Tsunami;

/// vote; requires at least two machines: a server and 1+ clients
#[instrument(name = "vote")]
pub(crate) async fn main() -> Result<(), Report> {
    let results = futures_util::future::join_all(vec![
        tokio::spawn(one(19, "skewed", 1, true).in_current_span()),
        tokio::spawn(one(19, "skewed", 6, true).in_current_span()),
        tokio::spawn(one(19, "skewed", 6, false).in_current_span()),
    ])
    .await;

    // surface any errors (if there are multiple, reports just the first, and that's fine)
    for result in results {
        result.unwrap()?;
    }
    Ok(())
}

#[instrument]
pub(crate) async fn one(
    write_every: usize,
    distribution: &'static str,
    nclients: usize,
    partial: bool,
) -> Result<(), Report> {
    let mut aws = crate::launcher();
    // aws.set_max_instance_duration(3);

    // try to ensure we do AWS cleanup
    let result: Result<(), Report> = try {
        tracing::info!("spinning up aws instances");
        let mut instances = vec![(
            String::from("server"),
            aws::Setup::default()
                .instance_type("r5n.4xlarge")
                .ami(crate::AMI, "ubuntu")
                .setup(crate::noria_setup("noria-server", "noria-server")),
        )];
        for clienti in 0..nclients {
            instances.push((
                format!("client{}", clienti),
                aws::Setup::default()
                    .instance_type("m5n.24xlarge")
                    .ami(crate::AMI, "ubuntu")
                    .setup(crate::noria_setup("noria-applications", "vote")),
            ));
        }
        aws.spawn(instances, Some(Duration::from_secs(2 * 60)))
            .await
            .wrap_err("failed to start instances")?;

        tracing::debug!("connecting");
        let vms = aws.connect_all().await?;
        let server = vms.get("server").unwrap();
        let s = server.ssh.as_ref().unwrap();
        let cs: Vec<_> = (0..nclients)
            .map(|clienti| {
                vms.get(&format!("client{}", clienti))
                    .unwrap()
                    .ssh
                    .as_ref()
                    .unwrap()
            })
            .collect();
        tracing::debug!("connected");

        let result: Result<(), Report> = try {
            let mut targets = ExponentialCliffSearcher::until(100_000, 400_000);
            while let Some(target) = targets.next() {
                let target_span = tracing::info_span!("target", target);
                async {
                    tracing::info!("start benchmark target");
                    let backend = if partial { "partial" } else { "full" };
                    let prefix = format!(
                        "{}.5000000a.{}t.{}r.{}c.{}",
                        backend, target, write_every, nclients, distribution,
                    );
                    let target_per_client = (target as f64 / nclients as f64).ceil() as usize;

                    tracing::trace!("starting noria server");
                    let mut noria_server = crate::server::build(s, server);
                    if !partial {
                        noria_server.arg("--no-partial");
                    }
                    let noria_server = noria_server
                        .arg("--durability=memory")
                        .arg("--no-reuse")
                        .arg("--shards=0")
                        .spawn()
                        .wrap_err("failed to start noria-server")?;

                    'run: {
                        tracing::debug!("prime");
                        let prime = vote_client(cs[0], server, |cmd| {
                            cmd.arg("--warmup=0")
                                .arg("--runtime=0")
                                .arg("-d")
                                .arg(distribution)
                                .arg("--articles=5000000")
                                .arg("--write-every")
                                .arg(write_every.to_string());
                        })
                        .stdout(std::process::Stdio::null())
                        .output()
                        .await
                        .wrap_err("failed to prime")?;

                        if !prime.status.success() {
                            tracing::warn!(
                                "priming failed:\n{}",
                                String::from_utf8_lossy(&prime.stderr)
                            );
                            targets.overloaded();
                            break 'run;
                        }

                        tracing::trace!("priming succeeded");
                        tracing::debug!("benchmark");
                        let mut benches = cs
                            .iter()
                            .copied()
                            .map(|c| {
                                vote_client(c, server, |cmd| {
                                    cmd.arg("--no-prime")
                                        .arg("--warmup=40")
                                        .arg("--runtime=40")
                                        .arg("--histogram=benchmark.hist")
                                        .arg("--target")
                                        .arg(target_per_client.to_string())
                                        .arg("-d")
                                        .arg(distribution)
                                        .arg("--articles=5000000")
                                        .arg("--write-every")
                                        .arg(write_every.to_string());
                                })
                                .stdout(std::process::Stdio::piped())
                                .stderr(std::process::Stdio::piped())
                                .spawn()
                            })
                            .collect::<Result<Vec<_>, _>>()
                            .wrap_err("failed to start client")?;

                        tracing::trace!("saving client output");
                        let results = tokio::fs::File::create(format!("{}.log", prefix));
                        let results = results.await.wrap_err("failed to create local log file")?;
                        let mut results = tokio::io::BufWriter::new(results);
                        let mut got_lines = false;
                        for bench in &mut benches {
                            let mut stdout =
                                tokio::io::BufReader::new(bench.stdout().take().unwrap()).lines();
                            while let Some(line) = stdout.next().await {
                                let line = line.wrap_err("failed to read client output")?;
                                results.write_all(line.as_bytes()).await?;
                                results.write_all(b"\n").await?;

                                if !line.starts_with('#') {
                                    let mut fields = line.split_whitespace();
                                    let field = fields.next().unwrap();
                                    let pct = fields.next();
                                    let sjrn = fields.next();

                                    if let (Some(pct), Some(sjrn)) = (pct, sjrn) {
                                        let pct: Result<u32, _> = pct.parse();
                                        let sjrn: Result<u32, _> = sjrn.parse();
                                        if let (Ok(pct), Ok(sjrn)) = (pct, sjrn) {
                                            got_lines = true;

                                            if pct == 50 && (sjrn > 100_000 || sjrn == 0) {
                                                tracing::warn!(
                                                    endpoint = field,
                                                    sojourn = sjrn,
                                                    "high sojourn latency"
                                                );
                                                targets.overloaded();
                                            }
                                            continue;
                                        }
                                    }
                                    tracing::warn!(case = "bad line", message = &*line);
                                } else if line.starts_with("# generated ops/s")
                                    | line.starts_with("# actual ops/s")
                                {
                                    let mut fields = line.split_whitespace();
                                    let rate: f64 = fields.next_back().unwrap().parse().unwrap();
                                    if target_per_client as f64 - rate
                                        > 0.05 * target_per_client as f64
                                    {
                                        tracing::warn!(%rate, "low throughput");
                                        targets.overloaded();
                                    }
                                }
                            }
                        }
                        results.flush().await?;

                        if !got_lines {
                            tracing::warn!("missing throughput line, probably overloaded");
                            targets.overloaded();
                        }

                        let mut all_ok = true;
                        let mut clients = Vec::new();
                        for (clienti, mut bench) in benches.into_iter().enumerate() {
                            use tokio::io::AsyncReadExt;
                            let mut stderr = String::new();
                            bench
                                .stderr()
                                .take()
                                .unwrap()
                                .read_to_string(&mut stderr)
                                .await?;
                            let status = bench.wait().await?;
                            if !status.success() {
                                tracing::warn!(client = clienti, "benchmark failed:\n{}", stderr);
                                targets.overloaded();
                                all_ok = false;
                            }
                            clients.push(status);
                        }
                        tracing::debug!("benchmark completed");

                        tracing::debug!("saving meta-info");
                        tracing::trace!("saving load metrics");
                        let (sload1, sload5) =
                            crate::load(s).await.wrap_err("failed to get server load")?;
                        results
                            .write_all(format!("# server load: {} {}\n", sload1, sload5).as_bytes())
                            .await?;
                        let vmrss = crate::server::vmrss(s)
                            .await
                            .wrap_err("failed to get server memory use")?;
                        results
                            .write_all(format!("# server memory (kB): {}\n", vmrss).as_bytes())
                            .await?;
                        let (cload1, cload5) = crate::load(cs[0])
                            .await
                            .wrap_err("failed to get client load")?;
                        results
                            .write_all(
                                format!("# client[0] load: {} {}\n", cload1, cload5).as_bytes(),
                            )
                            .await?;
                        results.flush().await?;
                        drop(results);

                        tracing::trace!("saving histograms");
                        for (clienti, &c) in cs.iter().enumerate() {
                            // only try to extract info about processes if things exited nicely
                            if !clients[clienti].success() {
                                tracing::trace!(client = clienti, "skipping failed client");
                                continue;
                            }

                            let client_span = tracing::debug_span!("histogram", client = clienti);
                            async {
                                tracing::trace!("saving histogram");
                                let mut histogram = c
                                    .sftp()
                                    .read_from("benchmark.hist")
                                    .await
                                    .wrap_err("failed to read remote histogram")?;
                                let mut results = tokio::fs::File::create(format!(
                                    "{}-client{}.hist",
                                    prefix, clienti
                                ))
                                .await
                                .wrap_err("failed to create local histogram copy")?;
                                tokio::io::copy(&mut histogram, &mut results)
                                    .await
                                    .wrap_err("failed to save remote histogram")?;
                                drop(results);
                                Ok::<_, Report>(())
                            }
                            .instrument(client_span)
                            .await?;
                        }

                        if all_ok {
                            tracing::trace!("saving server stats");
                            let mut results =
                                tokio::fs::File::create(format!("{}-statistics.json", prefix))
                                    .await
                                    .wrap_err("failed to create local stats file")?;
                            crate::server::write_stats(s, server, &mut results)
                                .await
                                .wrap_err("failed to save server stats")?;
                            results.flush().await?;
                            drop(results);
                            tracing::debug!("all results saved");
                        } else {
                            tracing::debug!("partial results saved");
                        }
                    }

                    tracing::debug!("stopping server");
                    crate::server::stop(s, noria_server).await?;
                    tracing::trace!("server stopped");

                    Ok::<_, Report>(())
                }
                .instrument(target_span)
                .await?;
            }
        };

        tracing::debug!("cleaning up");
        tracing::trace!("cleaning up ssh connections");
        for (name, host) in vms {
            let host_span = tracing::trace_span!("ssh_close", name = &*name);
            let ssh = host.ssh.expect("ssh connection to host disappeared");
            async {
                tracing::trace!("closing connection");
                if let Err(e) = ssh.close().await {
                    tracing::warn!("ssh connection failed: {}", e);
                }
            }
            .instrument(host_span)
            .await
        }

        result?
    };

    tracing::trace!("cleaning up instances");
    let cleanup = aws.terminate_all().await;
    tracing::debug!("done");
    let result = result?;
    let _ = cleanup.wrap_err("cleanup failed")?;
    Ok(result)
}

fn vote_client<'c>(
    ssh: &'c tsunami::Session,
    server: &'c tsunami::Machine<'c>,
    add_args: impl FnOnce(&mut openssh::Command<'_>),
) -> openssh::Command<'c> {
    let mut cmd = crate::noria_bin(ssh, "noria-applications", "vote");
    // vote args need to go _before_ the netsoup arguments
    add_args(&mut cmd);
    cmd.arg("netsoup")
        .arg("--deployment")
        .arg("benchmark")
        .arg("--zookeeper")
        .arg(format!(
            "{}:2181",
            server.private_ip.as_ref().expect("private ip unknown")
        ));
    cmd
}
