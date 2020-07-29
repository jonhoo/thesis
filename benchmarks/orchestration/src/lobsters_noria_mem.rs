use crate::Context;
use color_eyre::{eyre::WrapErr, Report};
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
    crate::explore!([(4000, 0)], one, ctx, false)
}

#[instrument(err, skip(ctx))]
pub(crate) async fn one(
    parameters: (usize, usize),
    limits: Option<Vec<usize>>,
    mut ctx: Context,
) -> Result<usize, Report> {
    let (scale, nshards) = parameters;
    let partial = true;
    let mut last_good_limit = 0;

    let mut aws = crate::launcher();
    aws.set_mode(aws::LaunchMode::on_demand());

    // try to ensure we do AWS cleanup
    let result: Result<_, Report> = try {
        tracing::info!("spinning up aws instances");

        aws.spawn(
            vec![
                (
                    String::from("server"),
                    aws::Setup::default()
                        .instance_type(&ctx.server_type)
                        .ami(crate::AMI, "ubuntu")
                        .availability_zone(ctx.az.clone())
                        .setup(crate::noria_setup("noria-server", "noria-server")),
                ),
                (
                    String::from("client"),
                    aws::Setup::default()
                        .instance_type(&ctx.client_type)
                        .ami(crate::AMI, "ubuntu")
                        .availability_zone(ctx.az.clone())
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
            Box::new(cliff::BinaryMinSearcher::until(1 * GB, 32 * MB))
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

                if limit == 64 * MB {
                    // this doesn't work
                    tracing::warn!(%limit, "skipping known-bad limit");
                    limits.overloaded();
                    continue;
                }

                if limit == 0 && scale % 500 == 0 && (scale / 500).is_power_of_two() {
                    // we already have this
                    tracing::info!(%scale, "skipping non-limited scale we already have");
                    continue;
                }

                if *ctx.exit.borrow() {
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
                    let mut noria_server = crate::server::build(s, server, None);
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

                    crate::invoke::lobsters::run(
                        &prefix,
                        scale,
                        || {
                            limits.overloaded();
                            successful_limit.take();
                        },
                        c,
                        &server,
                        &mut ctx,
                        true,
                    )
                    .await?;

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
