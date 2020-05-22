use color_eyre::Report;
use eyre::WrapErr;
use tracing::instrument;

pub(crate) fn build<'s>(
    ssh: &'s tsunami::Session,
    host: &'s tsunami::Machine<'s>,
) -> openssh::Command<'s> {
    // Set up the Noria server process
    let mut cmd = crate::noria_bin(ssh, "noria-server", "noria-server");
    cmd.arg("--deployment")
        .arg("benchmark")
        .arg("--address")
        .arg(host.private_ip.as_ref().expect("private ip unknown"));
    cmd
}

#[instrument(level = "trace", skip(ssh, server))]
pub(crate) async fn stop(
    ssh: &tsunami::Session,
    server: openssh::RemoteChild<'_>,
) -> Result<(), Report> {
    // Tell the server to shut down
    tracing::trace!("send stop signal");
    let _ = ssh
        .command("pkill")
        .arg("-o")
        .arg("noria-server")
        .status()
        .await
        .wrap_err("pkill")?;

    // Give it a little bit
    tokio::time::delay_for(std::time::Duration::from_secs(1)).await;

    // Check if it stopped normally
    tracing::trace!("check for termination");
    let status = server.wait();
    tokio::pin!(status);
    let status = match futures_util::poll!(&mut status) {
        std::task::Poll::Ready(Err(_)) => {
            // If we kill a remote process, the local ssh process will exit with an error, since
            // the remote command went away unexpectedly. This basically has to be an
            // openssh::Error::Disconnected. We want to still error if the entire ssh connection
            // went away, but if it's _just_ this one, we can keep going.
            if let Err(e) = ssh.check().await {
                Err(e)?
            } else {
                // All we can do is assume that the process exited successfully
                use std::os::unix::process::ExitStatusExt;
                std::process::ExitStatus::from_raw(0)
            }
        }
        std::task::Poll::Ready(Ok(status)) => {
            // Process had already exited before we sent the signal.
            status
        }
        std::task::Poll::Pending => {
            // It didn't stop -- force it to
            tracing::trace!("send kill signal");
            let _ = ssh
                .command("pkill")
                .arg("-o")
                .arg("-9")
                .arg("noria-server")
                .status()
                .await
                .wrap_err("pkill -9")?;

            tracing::trace!("wait for termination");
            match status.await {
                Ok(status) => status,
                Err(_) => {
                    // Same deal here -- error may just be Disconneted since the remote went away
                    if let Err(e) = ssh.check().await {
                        Err(e)?
                    } else {
                        // All we can do is assume that the process exited successfully
                        use std::os::unix::process::ExitStatusExt;
                        std::process::ExitStatus::from_raw(0)
                    }
                }
            }
        }
    };

    // Clean ZooKeeper state
    tracing::trace!("clean zookeeper state");
    let clean = crate::noria_bin(ssh, "noria-server", "noria-zk")
        .arg("--clean")
        .arg("--deployment")
        .arg("benchmark")
        .status()
        .await
        .wrap_err("noria-zk --clean")?;
    if !clean.success() {
        eyre::bail!("failed to clean zookeeper");
    }

    if !status.success() {
        eyre::bail!("noria-server exited with an error");
    }

    Ok(())
}

#[instrument(level = "trace", skip(ssh, w))]
pub(crate) async fn write_stats(
    ssh: &tsunami::Session,
    server: &tsunami::Machine<'_>,
    w: &mut (impl tokio::io::AsyncWrite + Unpin),
) -> Result<(), Report> {
    let mut curl = ssh
        .command("curl")
        .arg("-v")
        .arg(format!(
            "http://{}:6033/get_statistics",
            server.private_ip.as_ref().expect("private ip unknown")
        ))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .wrap_err("curl /get_statistics")?;

    let mut stderr = curl.stderr().take().unwrap();
    tokio::io::copy(&mut curl.stdout().as_mut().unwrap(), w)
        .await
        .wrap_err("failed to write curl output to local file")?;

    let status = curl.wait().await?;
    if !status.success() {
        use tokio::io::AsyncReadExt;
        let mut e = String::new();
        stderr.read_to_string(&mut e).await?;
        return Err(eyre::eyre!(e).wrap_err("failed to get server statistics"));
    }

    Ok(())
}

#[instrument(debug, skip(ssh))]
pub(crate) async fn vmrss(ssh: &tsunami::Session) -> Result<usize, Report> {
    let pid = crate::output_on_success(ssh.command("pgrep").arg("-o").arg("noria-server"))
        .await
        .wrap_err("pgrep")?;
    let pid = String::from_utf8_lossy(&pid.0);
    let pid: usize = match pid.trim().parse() {
        Ok(pid) => pid,
        Err(_) => Err(eyre::eyre!(pid.to_string()).wrap_err("failed to parse server pid"))?,
    };

    let vmrss = crate::output_on_success(ssh.shell(format!("grep VmRSS /proc/{}/status", pid)))
        .await
        .wrap_err("grep VmRSS")?;
    let vmrss = String::from_utf8_lossy(&vmrss.0);
    vmrss
        .split_whitespace()
        .nth(1)
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| eyre::eyre!(vmrss.to_string()).wrap_err("could not parse VmRSS"))
}
