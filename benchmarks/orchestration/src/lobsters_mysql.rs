use crate::Context;
use color_eyre::{eyre, eyre::WrapErr, Report};
use tokio::io::AsyncWriteExt;
use tracing::instrument;
use tracing_futures::Instrument;
use tsunami::providers::aws;
use tsunami::Tsunami;

const MYSQL_CONFIG: &str = "\
transaction_isolation = 'READ-UNCOMMITTED'
max_prepared_stmt_count = 131056
";

/// lobsters-mysql; requires two machines: a client and a server
#[instrument(name = "lobsters-mysql", skip(ctx))]
pub(crate) async fn main(ctx: Context) -> Result<(), Report> {
    let _ = one((), None, ctx).await?;
    Ok(())
}

#[instrument(err, skip(ctx))]
pub(crate) async fn one(
    _: (),
    loads: Option<Vec<usize>>,
    mut ctx: Context,
) -> Result<usize, Report> {
    let mut last_good_scale = 0;

    // make sure we shouldn't already be exiting.
    // this also sets it up so that _any_ recv from exit means we should exit.
    if let Some(false) = ctx.exit.recv().await {
    } else {
        tracing::info!("exiting as instructed");
        return Ok(0);
    }

    let mut aws = crate::launcher();
    aws.set_mode(aws::LaunchMode::on_demand());

    // try to ensure we do AWS cleanup
    let result: Result<_, Report> = try {
        tracing::info!("spinning up aws instances");

        fn s_setup<'r>(
            s: &'r tsunami::Machine<'_>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Report>> + Send + 'r>>
        {
            Box::pin(
                async move {
                    tracing::debug!("stop mysql (if running)");
                    crate::output_on_success(s.ssh.shell("sudo systemctl stop mariadb"))
                        .await
                        .wrap_err("stop mariadb")?;

                    tracing::debug!("mount mysql ramdisk");
                    crate::output_on_success(
                        s.ssh
                            .shell("sudo mount -t tmpfs -o size=60G tmpfs /var/lib/mysql"),
                    )
                    .await
                    .wrap_err("mount ramdisk")?;

                    tracing::debug!("install mysql configuration");
                    let mut config = String::from("[mysqld]\n");
                    config.push_str(MYSQL_CONFIG);
                    config.push_str("\n");
                    config.push_str(&format!(
                        "max-connections = {}\n",
                        crate::invoke::lobsters::IN_FLIGHT
                    ));
                    config.push_str(&format!(
                        "bind-address = {}\n",
                        s.private_ip.as_ref().expect("no private ip address?")
                    ));
                    let mut cmd = s
                        .ssh
                        .shell("sudo tee /etc/mysql/mariadb.conf.d/99-noria.cnf")
                        .stdout(std::process::Stdio::null())
                        .stdin(std::process::Stdio::piped())
                        .spawn()
                        .wrap_err("tee .cnf")?;
                    cmd.stdin()
                        .take()
                        .expect("set to piped above")
                        .write_all(config.as_bytes())
                        .await
                        .wrap_err("write .cnf")?;

                    let proc = cmd.wait_with_output().await.wrap_err("execute tee .cnf")?;
                    if !proc.status.success() {
                        Err(
                            eyre::eyre!(String::from_utf8_lossy(&proc.stderr).to_string())
                                .wrap_err("execute tee .cnf failed"),
                        )?;
                    }

                    tracing::debug!("install mysql main dbs");
                    crate::output_on_success(
                        s.ssh
                            .shell("sudo mysql_install_db --user=mysql --datadir=/var/lib/mysql"),
                    )
                    .await
                    .wrap_err("mysql_install_db")?;

                    tracing::debug!("start mysql");
                    crate::output_on_success(s.ssh.shell("sudo systemctl start mariadb"))
                        .await
                        .wrap_err("start mariadb")?;

                    tracing::debug!("make lobsters user");
                    crate::output_on_success(
                        s.ssh.shell("sudo mysql -e \"CREATE USER 'lobsters'\""),
                    )
                    .await
                    .wrap_err("create user")?;
                    tracing::trace!("grant all permissions");
                    crate::output_on_success(s.ssh.shell(
                        "sudo mysql -e \"GRANT ALL PRIVILEGES ON * . * TO 'lobsters'@'%';\"",
                    ))
                    .await
                    .wrap_err("grant all")?;
                    crate::output_on_success(s.ssh.shell("sudo mysql -e \"FLUSH PRIVILEGES\""))
                        .await
                        .wrap_err("flush privileges")?;

                    tracing::trace!("testing mysql setup");
                    crate::output_on_success(s.ssh.shell(&format!(
                        "mysql --protocol=TCP --user=lobsters --host={} -e \"SELECT 1\"",
                        s.private_ip.as_ref().unwrap()
                    )))
                    .await
                    .wrap_err("test mysql connection")?;

                    Ok(())
                }
                .in_current_span(),
            )
        }

        aws.spawn(
            vec![
                (
                    String::from("server"),
                    aws::Setup::default()
                        .instance_type(&ctx.server_type)
                        .ami(crate::AMI, "ubuntu")
                        .availability_zone(ctx.az.clone())
                        .setup(s_setup),
                ),
                (
                    String::from("client"),
                    aws::Setup::default()
                        .instance_type(&ctx.client_type)
                        .ami(crate::AMI, "ubuntu")
                        .availability_zone(ctx.az.clone())
                        .setup(crate::noria_setup("noria-applications", "lobsters-mysql")),
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
        let c = &client.ssh;
        tracing::debug!("connected");

        let mut scales = if let Some(loads) = loads {
            Box::new(cliff::LoadIterator::from(loads)) as Box<dyn cliff::CliffSearch + Send>
        } else {
            Box::new(cliff::ExponentialCliffSearcher::until(128, 32))
        };
        let result: Result<(), Report> = try {
            let mut successful_scale = None;
            while let Some(scale) = scales.next() {
                if let Some(scale) = successful_scale.take() {
                    // last run succeeded at the given scale
                    last_good_scale = scale;
                }
                successful_scale = Some(scale);

                if *ctx.exit.borrow() {
                    tracing::info!("exiting as instructed");
                    break;
                }

                let scale_span = tracing::info_span!("scale", scale);
                async {
                    tracing::info!("start benchmark target");
                    let prefix = format!("lobsters-mysql-{}-0m", scale);

                    // no need to start anything here -- we start MariaDB in setup
                    // and then the priming takes care of dropping/creating the DB.
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
                        false,
                    )
                    .await?;

                    // no need to stop anything either, for the same reason

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
