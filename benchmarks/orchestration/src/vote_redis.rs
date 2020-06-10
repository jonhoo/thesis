use crate::Context;
use color_eyre::Report;
use eyre::WrapErr;
use tracing::instrument;
use tracing_futures::Instrument;
use tsunami::providers::aws;
use tsunami::Tsunami;

/// vote; requires at least two machines: a server and 1+ clients
#[instrument(name = "vote-redis", skip(ctx))]
pub(crate) async fn main(ctx: Context) -> Result<(), Report> {
    crate::explore!(
        [
            //(20, "skewed", 1),
            //(20, "skewed", 1),
            //(2, "skewed", 6),
            //(2, "skewed", 6),
            (20, "skewed", 6),
            (20, "uniform", 6),
        ],
        one,
        ctx,
        false
    )
}

#[instrument(err, skip(ctx))]
pub(crate) async fn one(
    parameters: (usize, &'static str, usize),
    loads: Option<Vec<usize>>,
    mut ctx: Context,
) -> Result<usize, Report> {
    let (write_every, distribution, nclients) = parameters;
    let mut last_good_target = 0;

    let mut aws = crate::launcher();
    // vote exploration generally take less than two hours, but make it 3
    aws.set_max_instance_duration(3);

    fn redis_setup<'r>(
        _ssh: &'r mut tsunami::Session,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Report>> + Send + 'r>> {
        Box::pin(async { Ok(()) }.in_current_span())
    }

    // try to ensure we do AWS cleanup
    let result: Result<_, Report> = try {
        tracing::info!("spinning up aws instances");
        let mut instances = vec![(
            String::from("server"),
            aws::Setup::default()
                .instance_type(&ctx.server_type)
                .ami(crate::AMI, "ubuntu")
                .setup(redis_setup),
        )];
        for clienti in 0..nclients {
            instances.push((
                format!("client{}", clienti),
                aws::Setup::default()
                    .instance_type(&ctx.client_type)
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

        tracing::debug!("adjusting redis config");
        tracing::trace!("setting bind address");
        let adj = s
            .command("sudo")
            .arg("sed")
            .arg("-i")
            .arg("-e")
            .arg(format!(
                "s/^bind .*/bind {}/",
                server.private_ip.as_ref().expect("private ip unknown")
            ))
            .arg("-e")
            .arg("/^protected-mode yes/ s/yes/no/")
            .arg("/etc/redis/redis.conf")
            .status()
            .await
            .wrap_err("failed to adjust redis conf")?;
        if !adj.success() {
            eyre::bail!("redis conf sed");
        }

        let mut targets = if let Some(loads) = loads {
            Box::new(cliff::LoadIterator::from(loads)) as Box<dyn cliff::CliffSearch + Send>
        } else {
            Box::new(cliff::ExponentialCliffSearcher::until(100_000, 500_00))
        };
        let result: Result<(), Report> = try {
            let mut successful_target = None;
            while let Some(target) = targets.next() {
                if let Some(target) = successful_target.take() {
                    // last run succeeded at the given target
                    last_good_target = target;
                }
                successful_target = Some(target);

                if *ctx.exit.borrow() {
                    tracing::info!("exiting as instructed");
                    break;
                }

                let target_span = tracing::info_span!("target", target);
                async {
                    tracing::info!("start benchmark target");
                    let backend = "redis";
                    let prefix = format!(
                        "{}.5000000a.{}t.{}r.{}c.{}",
                        backend, target, write_every, nclients, distribution,
                    );

                    tracing::trace!("starting redis server");
                    let redis = s
                        .command("sudo")
                        .arg("systemctl")
                        .arg("restart") // restart in case it was already running
                        .arg("redis")
                        .status()
                        .await
                        .wrap_err("failed to start redis")?;
                    if !redis.success() {
                        eyre::bail!("systemctl start redis failed");
                    }

                    // give it a bit to start
                    tokio::time::delay_for(std::time::Duration::from_secs(3)).await;

                    crate::invoke::vote::run(
                        &prefix,
                        target,
                        distribution,
                        write_every,
                        || {
                            targets.overloaded();
                            successful_target.take();
                        },
                        &cs[..],
                        &server,
                        crate::invoke::vote::Backend::Redis,
                        &mut ctx,
                    )
                    .await?;

                    tracing::debug!("stopping server");
                    let flush = s
                        .command("redis-cli")
                        .arg("-h")
                        .arg(server.private_ip.as_ref().expect("private ip unknown"))
                        .arg("flushall")
                        .output()
                        .await
                        .wrap_err("failed to flush redis")?;
                    if !flush.status.success() {
                        return Err(
                            eyre::eyre!(String::from_utf8_lossy(&flush.stderr).to_string())
                                .wrap_err("failed to flush redis"),
                        );
                    }
                    let stop = s
                        .command("sudo")
                        .arg("systemctl")
                        .arg("stop")
                        .arg("redis")
                        .status()
                        .await
                        .wrap_err("failed to stop redis")?;
                    if !stop.success() {
                        eyre::bail!("systemctl stop redis failed");
                    }
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
    Ok(last_good_target)
}
