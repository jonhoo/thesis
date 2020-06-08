use crate::Context;
use color_eyre::Report;
use eyre::WrapErr;
use tracing::instrument;
use tracing_futures::Instrument;
use tsunami::providers::aws;
use tsunami::Tsunami;

/// lobsters-noria; requires two machines: a client and a server
#[instrument(name = "lobsters-noria", skip(ctx))]
pub(crate) async fn main(ctx: Context) -> Result<(), Report> {
    crate::explore!([(0, true), (0, false)], one, ctx, false)
}

#[instrument(err, skip(ctx))]
pub(crate) async fn one(
    parameters: (usize, bool),
    loads: Option<Vec<usize>>,
    mut ctx: Context,
) -> Result<usize, Report> {
    let (nshards, partial) = parameters;
    let mut last_good_scale = 0;

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
                        .instance_type(&ctx.server_type)
                        .ami(crate::AMI, "ubuntu")
                        .setup(crate::noria_setup("noria-server", "noria-server")),
                ),
                (
                    String::from("client"),
                    aws::Setup::default()
                        .instance_type(&ctx.client_type)
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

        let mut scales = if let Some(loads) = loads {
            Box::new(cliff::LoadIterator::from(loads)) as Box<dyn cliff::CliffSearch + Send>
        } else {
            Box::new(cliff::ExponentialCliffSearcher::until(500, 500))
        };
        let result: Result<(), Report> = try {
            let mut successful_scale = None;
            while let Some(scale) = scales.next() {
                if let Some(scale) = successful_scale.take() {
                    // last run succeeded at the given scale
                    last_good_scale = scale;
                }
                successful_scale = Some(scale);

                if (partial == false
                    && nshards == 0
                    && (scale == 8_000 || scale == 6_000 || scale == 5_000 || scale == 4_500))
                    || (partial == true && nshards == 0 && scale == 8_000)
                {
                    // i happen to know that this fails
                    scales.overloaded();
                    tracing::warn!(%scale, "skipping known-bad scale");
                    continue;
                }

                if *ctx.exit.borrow() {
                    tracing::info!("exiting as instructed");
                    break;
                }

                let scale_span = tracing::info_span!("scale", scale);
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
                    let prefix = format!("lobsters-{}-{}-0m", backend, scale);

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
                        .spawn()
                        .wrap_err("failed to start noria-server")?;

                    crate::invoke::lobsters::run(
                        &prefix,
                        scale,
                        || {
                            scales.overloaded();
                            successful_scale.take();
                        },
                        c,
                        &server,
                        &mut ctx,
                    )
                    .await?;

                    tracing::debug!("stopping server");
                    crate::server::stop(s, noria_server).await?;
                    tracing::trace!("server stopped");

                    Ok::<_, Report>(())
                }
                .instrument(scale_span)
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
    Ok(last_good_scale)
}
