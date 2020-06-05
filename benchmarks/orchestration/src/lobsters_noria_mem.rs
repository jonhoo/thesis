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

/// lobsters-noria; requires two machines: a client and a server
#[instrument(name = "lobsters-noria-mem", skip(ctx))]
pub(crate) async fn main(ctx: Context) -> Result<(), Report> {
    crate::explore!([(2000, 0), (3000, 0)], one, ctx, false)
}

#[instrument(err, skip(ctx))]
pub(crate) async fn one(
    parameters: (usize, usize),
    limits: Option<Vec<usize>>,
    ctx: Context,
) -> Result<usize, Report> {
    let (scale, nshards) = parameters;
    let partial = true;
    let mut last_good_limit = 0;
    let Context {
        server_type,
        client_type,
        mut exit,
        ..
    } = ctx;

    let mut aws = crate::launcher();
    // these actually take a while
    aws.set_max_instance_duration(3);

    // try to ensure we do AWS cleanup
    let result: Result<_, Report> = try {
        tracing::info!("spinning up aws instances");

        /*
        fn c_setup_patch<'r>(
            ssh: &'r mut tsunami::Session,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Report>> + Send + 'r>>
        {
            Box::pin(
                async move {
                    tracing::debug!("patch trawler");
                    crate::output_on_success(ssh.shell("cd noria && cargo update -p trawler"))
                        .await
                        .wrap_err("cargo update -p trawler")?;

                    crate::noria_setup("noria-applications", "lobsters-noria")(ssh).await?;

                    Ok(())
                }
                .in_current_span(),
            )
        }
        */

        aws.spawn(
            vec![
                (
                    String::from("server"),
                    aws::Setup::default()
                        .instance_type(&server_type)
                        .ami(crate::AMI, "ubuntu")
                        .setup(crate::noria_setup("noria-server", "noria-server")),
                ),
                (
                    String::from("client"),
                    aws::Setup::default()
                        .instance_type(&client_type)
                        .ami(crate::AMI, "ubuntu")
                        .setup(crate::noria_setup("noria-applications", "lobsters-noria")),
                ),
            ],
            None,
        )
        .await
        .wrap_err("failed to start instances")?;

        tracing::debug!("connecting");
        let vms = aws.connect_all().await?;
        let server = vms.get("server").unwrap();
        let client = vms.get("client").unwrap();
        let s = &server.ssh;
        let c = &client.ssh;
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

                if limit == 0 && scale % 500 == 0 && (scale / 500).is_power_of_two() {
                    // we already have this
                    tracing::info!(%scale, "skipping non-limited scale we already have");
                    continue;
                }

                if *exit.borrow() {
                    tracing::info!("exiting as instructed");
                    break;
                }

                let limit_span = tracing::info_span!("limit", limit);
                async {
                    tracing::info!("start benchmark target");
                    let mut backend = if nshards == 0 {
                        "direct".to_string()
                    } else {
                        format!("direct_{}", nshards)
                    };
                    if !partial {
                        backend.push_str("_full");
                    }
                    let prefix = format!("lobsters-{}-{}-{}m", backend, scale, limit);

                    tracing::trace!("starting noria server");
                    let mut noria_server = crate::server::build(s, server);
                    if !partial {
                        noria_server.arg("--no-partial");
                    }
                    let noria_server = noria_server
                        .arg("--durability=memory")
                        .arg("--no-reuse")
                        .arg("--shards")
                        .arg(nshards.to_string())
                        .arg("-m")
                        .arg(limit.to_string())
                        .spawn()
                        .wrap_err("failed to start noria-server")?;

                    'run: {
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
                                break 'run;
                            }
                        };

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
                        let mut stdout =
                            tokio::io::BufReader::new(bench.stdout().take().unwrap()).lines();
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
                                        target = Some(
                                            line.rsplitn(2, ' ').next().unwrap().parse::<f64>()?,
                                        );
                                    } else if line.starts_with("# generated ops/s") {
                                        actual = Some(
                                            line.rsplitn(2, ' ').next().unwrap().parse::<f64>()?,
                                        );
                                    }
                                    if let (Some(target), Some(actual)) = (target, actual) {
                                        if actual < target * 4.0 / 5.0 {
                                            tracing::warn!(%actual, %target, "low throughput");
                                            limits.overloaded();
                                            successful_limit.take();
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
                                        assert!(
                                            pct == "50" || pct == "99" || pct == "100",
                                            "{}",
                                            pct
                                        );
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
                                        tracing::warn!(
                                            endpoint = field,
                                            sojourn = us,
                                            "high sojourn latency"
                                        );
                                        limits.overloaded();
                                        successful_limit.take();
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
                                break 'run;
                            }
                        };

                        if target.is_none() || actual.is_none() {
                            tracing::warn!("missing throughput line, probably overloaded");
                            limits.overloaded();
                            successful_limit.take();
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
                            limits.overloaded();
                            successful_limit.take();
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
                        let (cload1, cload5) =
                            crate::load(c).await.wrap_err("failed to get client load")?;
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
    let _ = result?;
    let _ = cleanup.wrap_err("cleanup failed")?;
    Ok(last_good_limit)
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
