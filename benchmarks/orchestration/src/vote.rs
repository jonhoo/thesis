use crate::Context;
use color_eyre::{eyre::WrapErr, Report};
use tracing::instrument;
use tracing_futures::Instrument;
use tsunami::providers::aws;
use tsunami::Tsunami;

/// vote; requires at least two machines: a server and 1+ clients
#[instrument(name = "vote", skip(ctx))]
pub(crate) async fn main(ctx: Context) -> Result<(), Report> {
    crate::explore!(
        [
            // (100, "skewed", 4, false, 0, true, false),
            // (100, "skewed", 4, true, 0, true, false),
            (10_000, "skewed", 4, true, 0, false, false),
            (100, "skewed", 4, true, 256 * 1024 * 1024, true, false),
            (100, "skewed", 4, true, 320 * 1024 * 1024, true, false),
            (100, "skewed", 4, true, 384 * 1024 * 1024, true, false),
            (100, "skewed", 4, true, 448 * 1024 * 1024, true, false),
            (100, "skewed", 4, true, 448 * 1024 * 1024, true, true),
        ],
        one,
        ctx,
        false
    )
}

#[instrument(err, skip(ctx))]
pub(crate) async fn one(
    parameters: (usize, &'static str, usize, bool, usize, bool, bool),
    loads: Option<Vec<usize>>,
    mut ctx: Context,
) -> Result<usize, Report> {
    let (write_every, distribution, nclients, partial, memlimit, join, durable) = parameters;
    let mut last_good_target = 0;

    let mut aws = crate::launcher();
    aws.set_mode(aws::LaunchMode::on_demand());

    // try to ensure we do AWS cleanup
    let result: Result<_, Report> = try {
        tracing::info!("spinning up aws instances");
        let mut instances = vec![(
            String::from("server"),
            aws::Setup::default()
                .instance_type(&ctx.server_type)
                .ami(crate::AMI, "ubuntu")
                .availability_zone(ctx.az.clone())
                .setup(crate::noria_setup("noria-server", "noria-server")),
        )];
        for clienti in 0..nclients {
            instances.push((
                format!("client{}", clienti),
                aws::Setup::default()
                    .instance_type(&ctx.client_type)
                    .ami(crate::AMI, "ubuntu")
                    .availability_zone(ctx.az.clone())
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

        if durable {
            tracing::debug!("mount ramdisk");
            crate::output_on_success(s.shell("sudo mount -t tmpfs -o size=60G tmpfs /mnt"))
                .await
                .wrap_err("mount ramdisk")?;
        }

        let mut targets = if let Some(loads) = loads {
            Box::new(cliff::LoadIterator::from(loads)) as Box<dyn cliff::CliffSearch + Send>
        } else if durable {
            // all we care about is the 1M data point
            Box::new(cliff::LoadIterator::from(vec![1_000_000]))
                as Box<dyn cliff::CliffSearch + Send>
        } else if !partial {
            Box::new(cliff::LoadIterator::from(vec![250_000, 1_000_000]))
                as Box<dyn cliff::CliffSearch + Send>
        } else if write_every == 10_000 {
            let mut s = cliff::ExponentialCliffSearcher::until(1_000_000, 1_000_000);
            s.fill_left();
            Box::new(s)
        } else {
            let mut s = cliff::ExponentialCliffSearcher::until(250_000, 125_000);
            s.fill_left();
            Box::new(s)
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
                    let mut backend = if partial { "partial" } else { "full" }.to_string();
                    if !join {
                        backend.push_str("_nj");
                    }
                    if durable {
                        backend.push_str("_dur");
                    }
                    let prefix = format!(
                        "{}.10000000a.{}t.{}r.{}c.{}m.{}",
                        backend, target, write_every, nclients, memlimit, distribution,
                    );

                    tracing::trace!("starting noria server");
                    let dir = if durable { Some("/mnt") } else { None };
                    let mut noria_server = crate::server::build(s, server, dir);
                    if !partial {
                        noria_server.arg("--no-partial");
                    }
                    let durability = if durable {
                        "--durability=persistent"
                    } else {
                        "--durability=memory"
                    };
                    let noria_server = noria_server
                        .arg(durability)
                        .arg("--no-reuse")
                        .arg("--shards=0")
                        .arg("-m")
                        .arg(memlimit.to_string())
                        .spawn()
                        .wrap_err("failed to start noria-server")?;

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
                        crate::invoke::vote::Backend::Netsoup { join },
                        &mut ctx,
                    )
                    .await?;

                    if !*ctx.exit.borrow() {
                        tracing::debug!("stopping server");
                        crate::server::stop(s, noria_server).await?;
                        tracing::trace!("server stopped");
                    }

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
