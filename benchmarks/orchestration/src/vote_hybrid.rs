use crate::Context;
use color_eyre::{eyre, eyre::WrapErr, Report};
use tokio::io::AsyncWriteExt;
use tracing::instrument;
use tracing_futures::Instrument;
use tsunami::providers::aws;
use tsunami::Tsunami;

/// vote; requires at least two machines: a server and 1+ clients
#[instrument(name = "vote-hybrid", skip(ctx))]
pub(crate) async fn main(ctx: Context) -> Result<(), Report> {
    crate::explore!([(1000, "skewed", 6)], one, ctx, false)
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
                    config.push_str("\n");
                    config.push_str(&format!("max-connections = {}\n", 2000));
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

                    tracing::debug!("make vote user");
                    crate::output_on_success(s.ssh.shell("sudo mysql -e \"CREATE USER 'vote'\""))
                        .await
                        .wrap_err("create user")?;
                    tracing::trace!("grant all permissions");
                    crate::output_on_success(
                        s.ssh.shell(
                            "sudo mysql -e \"GRANT ALL PRIVILEGES ON * . * TO 'vote'@'%';\"",
                        ),
                    )
                    .await
                    .wrap_err("grant all")?;
                    crate::output_on_success(s.ssh.shell("sudo mysql -e \"FLUSH PRIVILEGES\""))
                        .await
                        .wrap_err("flush privileges")?;

                    tracing::trace!("testing mysql setup");
                    crate::output_on_success(s.ssh.shell(&format!(
                        "mysql --protocol=TCP --user=vote --host={} -e \"SELECT 1\"",
                        s.private_ip.as_ref().unwrap()
                    )))
                    .await
                    .wrap_err("test mysql connection")?;

                    Ok(())
                }
                .in_current_span(),
            )
        }

        let mut instances = vec![(
            String::from("server"),
            aws::Setup::default()
                .instance_type(&ctx.server_type)
                .ami(crate::AMI, "ubuntu")
                .availability_zone(ctx.az.clone())
                .setup(s_setup),
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
            Box::new(cliff::ExponentialCliffSearcher::until(100_000, 100_000))
        };
        let result: Result<(), Report> = try {
            let mut successful_target = None;
            while let Some(target) = targets.next() {
                if let Some(target) = successful_target.take() {
                    // last run succeeded at the given target
                    last_good_target = target;
                }
                successful_target = Some(target);

                if target == 100_000
                    || target == 200_000
                    || target == 400_000
                    || target == 800_000
                    || target == 1_200_000
                {
                    // skip ones we already have
                    tracing::warn!(%target, "skipping target we already have");
                    continue;
                }

                if target == 1_600_000 || target == 1_400_000 {
                    // i happen to know that this fails
                    targets.overloaded();
                    tracing::warn!(%target, "skipping known-bad target");
                    continue;
                }

                if *ctx.exit.borrow() {
                    tracing::info!("exiting as instructed");
                    break;
                }

                let target_span = tracing::info_span!("target", target);
                async {
                    tracing::info!("start benchmark target");
                    let backend = "hybrid";
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

                    // no need to start anything for MySQL -- we start MariaDB in setup
                    // and then the priming takes care of dropping/creating the DB.

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
                        crate::invoke::vote::Backend::Hybrid,
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

                    // no need to stop anything for MySQL either, for the same reason

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
