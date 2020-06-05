use crate::Context;
use color_eyre::Report;
use eyre::WrapErr;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    stream::StreamExt,
};
use tracing::instrument;
use tracing_futures::Instrument;
use tsunami::providers::aws;
use tsunami::Tsunami;

const KB: usize = 1024;
const MB: usize = 1024 * KB;
const GB: usize = 1024 * MB;

/// vote_mem; requires at least two machines: a server and 1+ clients
#[instrument(name = "vote-mem", skip(ctx))]
pub(crate) async fn main(ctx: Context) -> Result<(), Report> {
    crate::explore!(
        [(800_000, 20, "skewed", 6), (1_000_000, 20, "skewed", 6)],
        one,
        ctx,
        true
    )
}

#[instrument(err, skip(ctx))]
pub(crate) async fn one(
    parameters: (usize, usize, &'static str, usize),
    limits: Option<Vec<usize>>,
    ctx: Context,
) -> Result<usize, Report> {
    let (target, write_every, distribution, nclients) = parameters;
    let partial = true;
    let mut last_good_limit = 0;
    let Context {
        server_type,
        client_type,
        mut exit,
        ..
    } = ctx;

    let mut aws = crate::launcher();
    // vote exploration generally take less than an hour, but make it 2
    aws.set_max_instance_duration(2);

    // try to ensure we do AWS cleanup
    let result: Result<_, Report> = try {
        tracing::info!("spinning up aws instances");
        let mut instances = vec![(
            String::from("server"),
            aws::Setup::default()
                .instance_type(&server_type)
                .ami(crate::AMI, "ubuntu")
                .setup(crate::noria_setup("noria-server", "noria-server")),
        )];
        for clienti in 0..nclients {
            instances.push((
                format!("client{}", clienti),
                aws::Setup::default()
                    .instance_type(&client_type)
                    .ami(crate::AMI, "ubuntu")
                    .setup(crate::noria_setup("noria-applications", "vote")),
            ));
        }
        aws.spawn(instances, None)
            .await
            .wrap_err("failed to start instances")?;

        tracing::debug!("connecting");
        let vms = aws.connect_all().await?;
        let server = vms.get("server").unwrap();
        let s = &server.ssh;
        let cs: Vec<_> = (0..nclients)
            .map(|clienti| &vms.get(&format!("client{}", clienti)).unwrap().ssh)
            .collect();
        tracing::debug!("connected");

        let mut limits = if let Some(limits) = limits {
            Box::new(cliff::LoadIterator::from(limits)) as Box<dyn cliff::CliffSearch + Send>
        } else {
            Box::new(cliff::BinaryMinSearcher::until(2 * GB, 32 * MB))
                as Box<dyn cliff::CliffSearch + Send>
        };
        let mut zero = Some(0);
        let result: Result<(), Report> = try {
            let mut successful_limit = None;
            while let Some(limit) = zero.take().or_else(|| limits.next()) {
                if let Some(limit) = successful_limit.take() {
                    // last run succeeded at the given limit
                    last_good_limit = limit;
                }
                successful_limit = Some(limit);

                if limit == 0 && target % 1000 == 0 && (target / 1_000).is_power_of_two() {
                    // we already have this
                    tracing::info!(%target, "skipping non-limited target we already have");
                    continue;
                }

                if *exit.borrow() {
                    tracing::info!("exiting as instructed");
                    break;
                }

                let limit_span = tracing::info_span!("limit", limit);
                async {
                    tracing::info!("start benchmark target");
                    let backend = if partial { "partial" } else { "full" };
                    let prefix = format!(
                        "{}.5000000a.{}t.{}r.{}c.{}m.{}",
                        backend, target, write_every, nclients, limit, distribution,
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
                        .arg("-m")
                        .arg(limit.to_string())
                        .spawn()
                        .wrap_err("failed to start noria-server")?;

                    'run: {
                        tracing::debug!("prime");
                        let prime = vote_client(cs[0], server, |cmd| {
                            cmd
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
                            limits.overloaded();
                            successful_limit.take();
                            break 'run;
                        }

                        tracing::trace!("priming succeeded");

                        if *exit.borrow() {
                            break 'run;
                        }

                        tracing::debug!("benchmark");
                        let mut benches = cs
                            .iter()
                            .copied()
                            .map(|c| {
                                vote_client(c, server, |cmd| {
                                    cmd.arg("--no-prime")
                                        .arg("--runtime=540")
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
                        let fin = async {
                            for bench in &mut benches {
                                let mut stdout =
                                    tokio::io::BufReader::new(bench.stdout().take().unwrap())
                                        .lines();
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

                                                if pct == 50 && (sjrn > 200_000 || sjrn == 0) {
                                                    tracing::warn!(
                                                        endpoint = field,
                                                        sojourn = sjrn,
                                                        "high sojourn latency"
                                                    );
                                                    limits.overloaded();
                                                    successful_limit.take();
                                                }
                                                continue;
                                            }
                                        }
                                        tracing::warn!(case = "bad line", message = &*line);
                                    } else if line.starts_with("# generated ops/s")
                                        | line.starts_with("# actual ops/s")
                                    {
                                        let mut fields = line.split_whitespace();
                                        let rate: f64 =
                                            fields.next_back().unwrap().parse().unwrap();
                                        if target_per_client as f64 - rate
                                            > 0.05 * target_per_client as f64
                                        {
                                            tracing::warn!(%rate, bar = %target_per_client, "low throughput");
                                            limits.overloaded();
                                            successful_limit.take();
                                        }
                                    }
                                }
                            }
                            results.flush().await?;
                            Ok::<_, Report>(())
                        };

                        tokio::select! {
                            r = fin => {
                                let _ = r?;
                                if *exit.borrow() {
                                    break 'run;
                                }
                            }
                            _ = exit.recv() => {
                                break 'run;
                            }
                        };

                        if !got_lines {
                            tracing::warn!("missing throughput line, probably overloaded");
                            limits.overloaded();
                            successful_limit.take();
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
                                limits.overloaded();
                                successful_limit.take();
                                all_ok = false;
                            }
                            clients.push(status);
                        }
                        tracing::debug!("benchmark completed");

                        tracing::debug!("saving meta-info");
                        tracing::trace!("saving context");
                        results
                            .write_all(format!("# server type: {}\n", server_type).as_bytes())
                            .await?;
                        results
                            .write_all(format!("# client type: {}\n", client_type).as_bytes())
                            .await?;
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
                .instrument(limit_span)
                .await?;
            }
        };

        tracing::debug!("cleaning up");
        tracing::trace!("cleaning up ssh connections");
        for (name, host) in vms {
            let host_span = tracing::trace_span!("ssh_close", name = &*name);
            async {
                tracing::trace!("closing connection");
                if let Err(e) = host.ssh.close().await {
                    tracing::warn!("ssh connection failed: {:?}", e);
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
    let _ = result?;
    let _ = cleanup.wrap_err("cleanup failed")?;
    Ok(last_good_limit)
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
