use crate::Context;
use color_eyre::Report;
use eyre::WrapErr;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    stream::StreamExt,
};

pub(crate) async fn run(
    prefix: &str,
    scale: usize,
    mut on_overloaded: impl FnMut(),
    c: &tsunami::Session,
    server: &tsunami::Machine<'_>,
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
    let mut prime = lobsters_client(c, server, scale);
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
            return Ok(())
        }
    };

    if !prime.status.success() {
        tracing::warn!(
            "priming failed:\n{}",
            String::from_utf8_lossy(&prime.stderr)
        );
        on_overloaded();
        return Ok(());
    }

    tracing::trace!("priming succeeded");
    tracing::debug!("benchmark");
    let mut bench = lobsters_client(c, server, scale)
        .arg("--runtime=540")
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
                        tracing::warn!(%actual, %target, "low throughput");
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
                    tracing::warn!(case = "bad line", message = &*line);
                    continue;
                };
                if metric != "sojourn" {
                    assert_eq!(metric, "processing");
                    continue;
                }

                let pct = if let Some(pct) = fields.next() {
                    pct
                } else {
                    tracing::warn!(case = "bad line", message = &*line);
                    continue;
                };
                if pct != "95" {
                    assert!(pct == "50" || pct == "99" || pct == "100", "{}", pct);
                    continue;
                }

                let us = if let Some(us) = fields.next() {
                    us
                } else {
                    tracing::warn!(case = "bad line", message = &*line);
                    continue;
                };
                let us: usize = if let Ok(us) = us.parse() {
                    us
                } else {
                    tracing::warn!(case = "bad line", message = &*line);
                    continue;
                };
                if us > 200_000 {
                    tracing::warn!(endpoint = field, sojourn = us, "high sojourn latency");
                    on_overloaded();
                }
            }
        }
        results.flush().await?;
        Ok::<_, Report>(())
    };

    tokio::select! {
        r = fin => {
            let _ = r?;
        }
        _ = exit.recv() => {
            return Ok(());
        }
    };

    if target.is_none() || actual.is_none() {
        tracing::warn!("missing throughput line, probably overloaded");
        on_overloaded();
    }

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
    tracing::trace!("saving load metrics");
    let (sload1, sload5) = crate::load(s).await.wrap_err("failed to get server load")?;
    results
        .write_all(format!("# server load: {} {}\n", sload1, sload5).as_bytes())
        .await?;
    let vmrss = crate::server::vmrss(s)
        .await
        .wrap_err("failed to get server memory use")?;
    results
        .write_all(format!("# server memory (kB): {}\n", vmrss).as_bytes())
        .await?;
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

        tracing::trace!("saving server stats");
        let mut results = tokio::fs::File::create(format!("{}-statistics.json", prefix))
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

    Ok(())
}

fn lobsters_client<'c>(
    ssh: &'c tsunami::Session,
    server: &'c tsunami::Machine<'c>,
    scale: usize,
) -> openssh::Command<'c> {
    let mut cmd = crate::noria_bin(ssh, "noria-applications", "lobsters-noria");
    cmd.arg("--deployment")
        .arg("benchmark")
        .arg("-z")
        .arg(format!(
            "{}:2181",
            server.private_ip.as_ref().expect("private ip unknown")
        ))
        .arg("--scale")
        .arg(scale.to_string())
        .arg("--in-flight")
        .arg(256.to_string());
    cmd
}
