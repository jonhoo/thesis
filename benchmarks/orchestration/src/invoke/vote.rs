use crate::Context;
use color_eyre::{eyre::WrapErr, Report};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    stream::StreamExt,
};
use tracing_futures::Instrument;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Backend {
    Netsoup { join: bool },
    Redis,
    Hybrid,
}

pub(crate) async fn run(
    prefix: &str,
    target: usize,
    distribution: &str,
    write_every: usize,
    mut on_overloaded: impl FnMut(),
    cs: &[&openssh::Session],
    server: &tsunami::Machine<'_>,
    backend: Backend,
    ctx: &mut Context,
) -> Result<(), Report> {
    let Context {
        ref server_type,
        ref client_type,
        ref mut exit,
        ..
    } = *ctx;

    let s = &server.ssh;
    let target_per_client = (target as f64 / cs.len() as f64).ceil() as usize;

    tracing::debug!("prime");
    let prime = vote_client(cs[0], server, backend, |cmd| {
        cmd.arg("--runtime=60")
            .arg("--target=500000") // also warm a bit
            .arg("-d")
            .arg(distribution)
            .arg("--articles=10000000")
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
        on_overloaded();
        return Ok(());
    }

    tracing::trace!("priming succeeded");

    if *exit.borrow() {
        return Ok(());
    }

    tracing::debug!("benchmark");
    let mut benches = cs
        .iter()
        .map(|c| {
            vote_client(c, server, backend, |cmd| {
                cmd.arg("--no-prime")
                    .arg("--runtime=288")
                    .arg("--histogram=benchmark.hist")
                    .arg("--target")
                    .arg(target_per_client.to_string())
                    .arg("-d")
                    .arg(distribution)
                    .arg("--articles=10000000")
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
            let mut stdout = tokio::io::BufReader::new(bench.stdout().take().unwrap()).lines();
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

                            if pct == 90 && sjrn > 20_000 {
                                tracing::error!(
                                    endpoint = field,
                                    latency = sjrn,
                                    "high sojourn latency"
                                );
                                on_overloaded();
                            }
                            continue;
                        }
                    }
                    tracing::error!(case = "bad line", message = &*line);
                } else if line.starts_with("# generated ops/s") | line.starts_with("# actual ops/s")
                {
                    let mut fields = line.split_whitespace();
                    let rate: f64 = fields.next_back().unwrap().parse().unwrap();
                    if target_per_client as f64 - rate > 0.05 * target_per_client as f64 {
                        tracing::error!(%rate, bar = %target_per_client, "low throughput");
                        on_overloaded();
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
                return Ok(());
            }
        }
        _ = exit.recv() => {
            return Ok(());
        }
    };

    if !got_lines {
        tracing::warn!("missing throughput line, probably overloaded");
        on_overloaded();
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
            on_overloaded();
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
    let commit = crate::noria_commit(s)
        .await
        .wrap_err("failed to get noria commit")?;
    results
        .write_all(format!("# commit: {}\n", commit).as_bytes())
        .await?;
    tracing::trace!("saving load metrics");
    let (sload1, sload5) = crate::load(s).await.wrap_err("failed to get server load")?;
    results
        .write_all(format!("# server load: {} {}\n", sload1, sload5).as_bytes())
        .await?;
    if sload1 > 15.5 {
        tracing::warn!(%sload1, "high server load -- assuming overloaded");
        on_overloaded();
    }

    let vmrss_for = match backend {
        Backend::Netsoup { .. } => "noria-server",
        Backend::Redis => "redis-server",
        Backend::Hybrid => {
            let vmrss = crate::server::vmrss_for(s, "mysqld")
                .await
                .wrap_err("failed to get MySQL memory use")?;
            results
                .write_all(format!("# backend memory (kB): {}\n", vmrss).as_bytes())
                .await?;
            "redis-server"
        }
    };
    let vmrss = crate::server::vmrss_for(s, vmrss_for)
        .await
        .wrap_err("failed to get server memory use");
    match vmrss {
        Ok(vmrss) => {
            results
                .write_all(format!("# server memory (kB): {}\n", vmrss).as_bytes())
                .await?;
        }
        Err(e) => {
            // the server process probably crashed
            let _ = server
                .ssh
                .check()
                .await
                .wrap_err("check after vmrss failure")?;
            // connection still good, so just mark this as bad and move on
            tracing::warn!("{:?}", e);
            on_overloaded();
        }
    }

    let (cload1, cload5) = crate::load(cs[0])
        .await
        .wrap_err("failed to get client load")?;
    results
        .write_all(format!("# client[0] load: {} {}\n", cload1, cload5).as_bytes())
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
            let mut results = tokio::fs::File::create(format!("{}-client{}.hist", prefix, clienti))
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
        if let Backend::Netsoup { .. } = backend {
            tracing::trace!("saving server stats");
            let mut results = tokio::fs::File::create(format!("{}-statistics.json", prefix))
                .await
                .wrap_err("failed to create local stats file")?;
            let timed_out = crate::server::write_stats(s, server, &mut results)
                .await
                .wrap_err("failed to save server stats")?;
            if timed_out {
                tracing::warn!("timed out when fetching server stats");
                on_overloaded();
            }
            results.flush().await?;
            drop(results);
        }
        tracing::debug!("all results saved");
    } else {
        tracing::debug!("partial results saved");
    }

    Ok(())
}

fn vote_client<'c>(
    ssh: &'c openssh::Session,
    server: &'c tsunami::Machine<'c>,
    backend: Backend,
    add_args: impl FnOnce(&mut openssh::Command<'_>),
) -> openssh::Command<'c> {
    let mut cmd = crate::noria_bin(ssh, "vote");
    // vote args need to go _before_ the backend arguments
    add_args(&mut cmd);
    match backend {
        Backend::Netsoup { join } => {
            cmd.arg("netsoup")
                .arg("--deployment")
                .arg("benchmark")
                .arg("--zookeeper")
                .arg(format!(
                    "{}:2181",
                    server.private_ip.as_ref().expect("private ip unknown")
                ));
            if !join {
                cmd.arg("--no-join");
            }
        }
        Backend::Redis => {
            cmd.arg("redis")
                .arg("--address")
                .arg(server.private_ip.as_ref().expect("private ip unknown"));
        }
        Backend::Hybrid => {
            cmd.arg("hybrid")
                .arg("--mysql-address")
                .arg(format!(
                    "vote@{}/soup",
                    server.private_ip.as_ref().expect("private ip unknown")
                ))
                .arg("--redis-address")
                .arg(server.private_ip.as_ref().expect("private ip unknown"));
        }
    }
    cmd
}
