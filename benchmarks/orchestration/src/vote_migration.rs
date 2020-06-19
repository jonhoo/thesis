use crate::Context;
use color_eyre::Report;
use eyre::WrapErr;
use tracing::instrument;
use tracing_futures::Instrument;
use tsunami::providers::aws;
use tsunami::providers::Launcher;

/// vote-migration; requires only one machine
#[instrument(err, name = "vote_migration", skip(ctx))]
pub(crate) async fn main(ctx: Context) -> Result<(), Report> {
    let Context {
        server_type,
        mut exit,
        ..
    } = ctx;

    let mut aws = crate::launcher();
    // shouldn't take _that_ long
    aws.set_max_instance_duration(1);

    // try to ensure we do AWS cleanup
    let result: Result<(), Report> = try {
        tracing::info!("spinning up aws instances");
        aws.spawn(
            vec![(
                String::from("host"),
                aws::Setup::default()
                    .instance_type(&server_type)
                    .ami(crate::AMI, "ubuntu")
                    .setup(crate::noria_setup("noria-applications", "vote-migration")),
            )],
            None,
        )
        .await
        .wrap_err("failed to start instances")?;

        tracing::debug!("connecting");
        let vms = aws.connect_all().await?;
        let host = vms.get("host").unwrap();
        let ssh = &host.ssh;
        tracing::debug!("connected");

        tracing::info!("running benchmark");

        // work around https://github.com/rust-lang/rust/issues/48594#issuecomment-632729902
        async {
            'run: {
                tracing::trace!("launching remote process");
                let mut benchmark = crate::noria_bin(ssh, "noria-applications", "vote-migration");
                benchmark
                    .arg("--migrate=90")
                    .arg("--runtime=180")
                    .arg("--do-it-all")
                    .arg("--articles=10000000")
                    .stdout(std::process::Stdio::null());
                let benchmark = crate::output_on_success(benchmark);

                // make sure we shouldn't already be exiting.
                // this also sets it up so that _any_ recv from exit means we should exit.
                if let Some(false) = exit.recv().await {
                } else {
                    tracing::info!("exiting as instructed");
                    break 'run;
                }

                tokio::select! {
                    r = benchmark => {
                        let _ = r?;
                    }
                    _ = exit.recv() => {
                        tracing::info!("exiting as instructed");
                        break 'run;
                    }
                };

                tracing::info!("benchmark completed successfully");

                // copy out all the log files
                let files = ssh
                    .command("ls")
                    .raw_arg("vote-*.log")
                    .output()
                    .await
                    .wrap_err("ls vote-*.log")?;
                if files.status.success() {
                    let mut sftp = ssh.sftp();
                    let mut nfiles = 0;
                    tracing::debug!("downloading log files");
                    for file in std::io::BufRead::lines(&*files.stdout) {
                        let file = file.expect("reading from Vec<u8> cannot fail");
                        let file_span = tracing::trace_span!("file", file = &*file);
                        async {
                            tracing::trace!("downloading");
                            let mut remote = sftp
                                .read_from(&file)
                                .in_current_span()
                                .await
                                .wrap_err("open remote file")?;
                            let mut local = tokio::fs::File::create(&file)
                                .in_current_span()
                                .await
                                .wrap_err("create local file")?;
                            tokio::io::copy(&mut remote, &mut local)
                                .in_current_span()
                                .await
                                .wrap_err("copy remote to local")?;

                            nfiles += 1;
                            Ok::<_, Report>(())
                        }
                        .instrument(file_span)
                        .await?;
                    }
                    tracing::debug!(n = nfiles, "log files downloaded");
                }
            }
            Ok::<_, Report>(())
        }
        .await?;

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
    };

    tracing::trace!("cleaning up instances");
    let cleanup = aws.terminate_all().await;
    tracing::debug!("done");
    let result = result?;
    let _ = cleanup.wrap_err("cleanup failed")?;
    Ok(result)
}
