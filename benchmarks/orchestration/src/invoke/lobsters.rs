use crate::Context;
use color_eyre::{eyre::WrapErr, Report};
use std::time::Instant;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    stream::StreamExt,
};

pub(crate) const IN_FLIGHT: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Backend {
    Noria,
    Mysql { optimized: bool },
}

pub(crate) async fn run(
    prefix: &str,
    scale: usize,
    mut on_overloaded: impl FnMut(),
    c: &openssh::Session,
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

    tracing::debug!("prime");
    let mut prime = lobsters_client(c, server, scale, backend);
    let prime_start = Instant::now();
    let prime = prime
        .arg("--runtime=0")
        .arg("--prime")
        .stdout(std::process::Stdio::null())
        .output();

    // priming in lobsters-noria is slow, so allow interrupting with ctrl-c
    let prime = tokio::select! {
        r = prime => {
            r.wrap_err("failed to prime")?
        }
        _ = exit.recv() => {
            tracing::warn!("exiting priming early as requested");
            return Ok(())
        }
    };
    let prime_took = prime_start.elapsed();

    if !prime.status.success() {
        tracing::warn!(
            "priming failed:\n{}",
            String::from_utf8_lossy(&prime.stderr)
        );
        on_overloaded();
        return Ok(());
    }

    tracing::trace!(time = ?prime_took, "priming succeeded");

    if *exit.borrow() {
        return Ok(());
    }

    tracing::debug!("benchmark");
    let mut bench = lobsters_client(c, server, scale, backend)
        .arg("--runtime=320")
        .arg("--histogram=benchmark.hist")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .wrap_err("failed to start client")?;

    // save normal output
    tracing::trace!("saving client output");
    let mut stdout = tokio::io::BufReader::new(bench.stdout().take().unwrap()).lines();
    let results = tokio::fs::File::create(format!("{}.log", prefix));
    let results = results.await.wrap_err("failed to create local log file")?;
    let mut results = tokio::io::BufWriter::new(results);
    let mut target = None;
    let mut actual = None;
    let fin = async {
        while let Some(line) = stdout.next().await {
            let line = line.wrap_err("failed to read client output")?;
            // println!("{}", line);
            results.write_all(line.as_bytes()).await?;
            results.write_all(b"\n").await?;

            if target.is_none() || actual.is_none() {
                if line.starts_with("# target ops/s") {
                    target = Some(line.rsplitn(2, ' ').next().unwrap().parse::<f64>()?);
                } else if line.starts_with("# generated ops/s") {
                    actual = Some(line.rsplitn(2, ' ').next().unwrap().parse::<f64>()?);
                }
                if let (Some(target), Some(actual)) = (target, actual) {
                    if actual < target * 4.0 / 5.0 {
                        tracing::error!(%actual, %target, "low throughput");
                        on_overloaded();
                    }
                }
            }

            // Submit          sojourn         95      4484
            if line.contains("sojourn") {
                let mut fields = line.trim().split_whitespace();
                let field = fields.next().unwrap();
                if let "Login" | "Logout" = field {
                    // ignore not-that-interesting endpoints
                    continue;
                }

                let metric = if let Some(metric) = fields.next() {
                    metric
                } else {
                    tracing::error!(case = "bad line", message = &*line);
                    continue;
                };
                if metric != "sojourn" {
                    assert_eq!(metric, "processing");
                    continue;
                }

                let pct = if let Some(pct) = fields.next() {
                    pct
                } else {
                    tracing::error!(case = "bad line", message = &*line);
                    continue;
                };
                if pct != "50" {
                    assert!(pct == "95" || pct == "99" || pct == "100", "{}", pct);
                    continue;
                }

                let us = if let Some(us) = fields.next() {
                    us
                } else {
                    tracing::error!(case = "bad line", message = &*line);
                    continue;
                };
                let us: f64 = if let Ok(us) = us.parse() {
                    us
                } else {
                    tracing::error!(case = "bad line", message = &*line);
                    continue;
                };
                if us > 200_000.0 {
                    tracing::error!(endpoint = field, sojourn = %us, "high sojourn latency");
                    on_overloaded();
                }
            }
        }
        results.flush().await?;
        Ok::<_, Report>(())
    };

    tracing::trace!("grabbing stderr");
    let eh = bench.stderr().take().unwrap();
    let stderr = tokio::spawn(async move {
        let mut eh = tokio::io::BufReader::new(eh).lines();
        let mut stderr = String::new();
        while let Some(line) = eh.next().await {
            let line = line.wrap_err("failed to read client stderr")?;
            // eprintln!("{}", line);
            stderr.push_str(&line);
            stderr.push_str("\n");
        }
        Ok::<_, Report>(stderr)
    });

    tokio::select! {
        r = fin => {
            let _ = r?;
            if *exit.borrow() {
                return Ok(());
            }
        }
        _ = exit.recv() => {
            tracing::warn!("exiting benchmark early as requested");
            return Ok(());
        }
    };

    if target.is_none() || actual.is_none() {
        tracing::warn!("missing throughput line, probably overloaded");
        on_overloaded();
    }

    tracing::trace!("gathering stderr");
    let stderr = stderr.await.unwrap()?;
    tracing::debug!("waiting for benchmark to terminate");
    let status = bench.wait().await?;
    if !status.success() {
        tracing::warn!("benchmark failed:\n{}", stderr);
        on_overloaded();
    }

    tracing::debug!("saving meta-info");
    tracing::trace!("saving context");
    results
        .write_all(format!("# server type: {}\n", server_type).as_bytes())
        .await?;
    results
        .write_all(format!("# client type: {}\n", client_type).as_bytes())
        .await?;
    results
        .write_all(format!("# prime time: {}\n", prime_took.as_secs_f64()).as_bytes())
        .await?;
    tracing::trace!("saving commit");
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
        tracing::warn!(%sload5, "high server load -- assuming overloaded");
        on_overloaded();
    }

    let vmrss_for = if let Backend::Noria = backend {
        "noria-server"
    } else {
        "mysqld"
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
    let (cload1, cload5) = crate::load(c).await.wrap_err("failed to get client load")?;
    results
        .write_all(format!("# client load: {} {}\n", cload1, cload5).as_bytes())
        .await?;
    results.flush().await?;
    drop(results);

    // only try to extract info about processes if things exited nicely
    if status.success() {
        tracing::trace!("saving histogram");
        let mut histogram = c
            .sftp()
            .read_from("benchmark.hist")
            .await
            .wrap_err("failed to read remote histogram")?;
        let mut results = tokio::fs::File::create(format!("{}.hist", prefix))
            .await
            .wrap_err("failed to create local histogram copy")?;
        tokio::io::copy(&mut histogram, &mut results)
            .await
            .wrap_err("failed to save remote histogram")?;
        drop(results);

        if let Backend::Noria = backend {
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
        }
        tracing::debug!("all results saved");
    } else {
        tracing::debug!("partial results saved");
    }

    Ok(())
}

fn lobsters_client<'c>(
    ssh: &'c openssh::Session,
    server: &'c tsunami::Machine<'c>,
    scale: usize,
    backend: Backend,
) -> openssh::Command<'c> {
    let mut cmd = match backend {
        Backend::Noria => {
            let mut cmd = crate::noria_bin(ssh, "lobsters-noria");
            cmd.arg("--deployment")
                .arg("benchmark")
                .arg("-z")
                .arg(format!(
                    "{}:2181",
                    server.private_ip.as_ref().expect("private ip unknown")
                ));
            cmd
        }
        Backend::Mysql { optimized } => {
            let mut cmd = crate::noria_bin(ssh, "lobsters-mysql");
            cmd.arg("--queries");
            if optimized {
                cmd.arg("original");
            } else {
                cmd.arg("natural");
            }
            cmd.arg(format!(
                "mysql://lobsters@{}/soup",
                server.private_ip.as_ref().expect("private ip unknown")
            ));
            cmd
        }
    };
    cmd.arg("--scale")
        .arg(scale.to_string())
        .arg("--in-flight")
        .arg(IN_FLIGHT.to_string());
    cmd
}
