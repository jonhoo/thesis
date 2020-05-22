#![feature(try_blocks, label_break_value)]

const AMI: &str = "ami-037890a1186dbfcb8";

use clap::{App, Arg};
use color_eyre::Report;
use eyre::WrapErr;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use tracing::instrument;
use tracing_futures::Instrument;
use tsunami::providers::aws;

mod lobsters_noria;
mod vote;
mod vote_migration;

pub(crate) mod server;

#[tokio::main]
async fn main() {
    let mut benchmarks = vec!["vote-migration", "vote", "lobsters-noria"];

    let matches = App::new("Noria benchmark orchestrator")
        .author("Jon Gjengset <jon@tsp.io>")
        .about("Run Noria benchmarks on EC2")
        .arg(
            Arg::with_name("benchmarks")
                .index(1)
                .multiple(true)
                .possible_values(&benchmarks)
                .help("Run only the specified benchmarks [all by default]"),
        )
        .get_matches();

    // only run specified benchmarks
    if let Some(vs) = matches.values_of("benchmarks") {
        benchmarks.clear();
        benchmarks.extend(vs);
    }

    // set up tracing
    use tracing_error::ErrorLayer;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};
    let fmt_layer = fmt::layer().with_target(false);
    let filter_layer = EnvFilter::from_default_env();
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .with(ErrorLayer::default())
        .init();

    // run all benchmarks in parallel
    tracing::info!("starting all benchmarks");
    let mut running = BTreeMap::new();
    for benchmark in benchmarks {
        let had = running.insert(
            benchmark,
            match benchmark {
                "vote-migration" => tokio::spawn(vote_migration::main()),
                "vote" => tokio::spawn(vote::main()),
                "lobsters-noria" => tokio::spawn(lobsters_noria::main()),
                _ => unreachable!("{}", benchmark),
            },
        );
        assert!(had.is_none());
    }
    tracing::trace!("all benchmarks started");

    // wait for all to complete before reporting any results
    tracing::trace!("waiting for benchmarks to complete");
    let mut completed = BTreeMap::new();
    for (benchmark, completion) in running {
        let result = completion.await.expect("runtime shut down");
        if let Err(ref e) = result {
            tracing::error!(%benchmark, "benchmark failed: {}", e);
        } else {
            tracing::debug!(%benchmark, "benchmark completed");
        }
        completed.insert(benchmark, result);
    }
    tracing::info!("all benchmarks completed");

    // show result of all benchmarks
    for (_, result) in completed {
        if let Err(e) = result {
            // NOTE: benchmark name is already in spans
            eprintln!("{:?}", e);
        }
    }
}

fn launcher() -> aws::Launcher<rusoto_sts::StsAssumeRoleSessionCredentialsProvider> {
    aws::Launcher::default().with_credentials(|| {
        let sts = rusoto_sts::StsClient::new(rusoto_core::Region::UsEast1);
        Ok(rusoto_sts::StsAssumeRoleSessionCredentialsProvider::new(
            sts,
            "arn:aws:sts::125163634912:role/soup".to_owned(),
            "jon-thesis".to_owned(),
            None,
            None,
            None,
            None,
        ))
    })
}

/// Prepare a box to run a particular experiment.
///
/// Note that we _generate_ a setup function, so that the setup can differ per experiment.
#[instrument(debug)]
fn noria_setup(
    package: &'static str,
    binary: &'static str,
) -> Box<
    dyn for<'r> Fn(
            &'r mut tsunami::Session,
        ) -> Pin<Box<dyn Future<Output = Result<(), Report>> + Send + 'r>>
        + Send
        + Sync
        + 'static,
> {
    Box::new(move |ssh| {
        Box::pin(
            async move {
                // first, make sure we have the latest release
                tracing::debug!("setting up host");
                tracing::trace!("git pull");
                let updated = ssh
                    .shell("git -C noria pull")
                    .status()
                    .await
                    .wrap_err("git pull")?;
                if !updated.success() {
                    eyre::bail!("git pull failed");
                }

                // then, we need to compile the target binary
                tracing::trace!("cargo build");
                let compiled = ssh
                    .shell(format!(
                        "cd noria && cargo b -p {} --bin {} --release",
                        package, binary
                    ))
                    .status()
                    .await
                    .wrap_err("cargo build")?;
                if !compiled.success() {
                    eyre::bail!("failed to compile")
                }

                // and then ensure that ZooKeeper is running
                tracing::trace!("start zookeeper");
                let zk = ssh
                    .shell("sudo systemctl start zookeeper")
                    .status()
                    .await
                    .wrap_err("start zookeeper")?;
                if !zk.success() {
                    eyre::bail!("failed to start zookeeper")
                }

                tracing::debug!("setup complete");
                Ok(())
            }
            .in_current_span(),
        )
    })
}

fn noria_bin<'s>(
    ssh: &'s tsunami::Session,
    package: &'static str,
    binary: &'static str,
) -> openssh::Command<'s> {
    let mut cmd = ssh.command("cargo");
    cmd.arg("+nightly")
        .arg("run")
        .arg("--manifest-path=noria/Cargo.toml")
        .arg("-p")
        .arg(package)
        .arg("--release")
        .arg("--bin")
        .arg(binary)
        .arg("--");
    cmd
}

#[instrument(debug, skip(cmd))]
async fn output_on_success<'a, C: std::borrow::BorrowMut<openssh::Command<'a>>>(
    mut cmd: C,
) -> Result<(Vec<u8>, Vec<u8>), Report> {
    let proc = cmd
        .borrow_mut()
        .output()
        .await
        .wrap_err("failed to execute")?;
    if proc.status.success() {
        Ok((proc.stdout, proc.stderr))
    } else {
        Err(
            eyre::eyre!(String::from_utf8_lossy(&proc.stderr).to_string())
                .wrap_err("execution failed"),
        )
    }
}

#[instrument(debug, skip(ssh))]
pub(crate) async fn load(ssh: &tsunami::Session) -> Result<(f64, f64), Report> {
    let load = crate::output_on_success(
        ssh.command("awk")
            .arg("{print $1\" \"$2}")
            .arg("/proc/loadavg"),
    )
    .await
    .wrap_err("awk")?;

    let load = String::from_utf8_lossy(&load.0);

    let mut loads = load
        .split_whitespace()
        .map(|c| -> Result<f64, _> { c.parse() });

    if let Some(Ok(load1)) = loads.next() {
        if let Some(Ok(load5)) = loads.next() {
            return Ok((load1, load5));
        }
    }

    Err(eyre::eyre!(load.to_string())).wrap_err("bad load")
}
