//! Headless share for the share lab: shares whatever Ket tunnel is up with
//! nearby devices, exactly as the desktop app does, until SIGTERM.
//!
//!   share_check --address IPV4 --credentials FILE
//!
//! Runs unprivileged, like the desktop app. Writes `PORT USER PASSWORD` to
//! FILE (created with mode 0600) and prints only `sharing port=…`, then
//! `status devices=… refused=… tunnel=…` every 2 seconds and `stopped` (or
//! `ended network_gone`); the password never reaches the output.

use std::{
    fs::OpenOptions, io::Write, net::Ipv4Addr, path::PathBuf, process::ExitCode, time::Duration,
};

use ket_client_core::{ShareEnd, ShareSession};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("share_check: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let mut address = None;
    let mut credentials = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("{argument} needs a value"))?;
        match argument.as_str() {
            "--address" => {
                address = Some(
                    value
                        .parse::<Ipv4Addr>()
                        .map_err(|_| "--address needs an IPv4 address".to_owned())?,
                );
            }
            "--credentials" => credentials = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown option {argument}")),
        }
    }
    let address = address.ok_or("--address is required")?;
    let credentials = credentials.ok_or("--credentials is required")?;

    let session = ShareSession::start(address)
        .await
        .map_err(|error| format!("{} ({})", error, error.code()))?;
    let details = session.details();
    write_private(
        &credentials,
        &format!(
            "{} {} {}\n",
            details.port, details.username, details.password
        ),
    )
    .map_err(|error| format!("credentials file: {error}"))?;
    println!("sharing port={}", details.port);

    let mut ticker = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let status = session.status();
                if let Some(ended) = status.ended {
                    match ended {
                        ShareEnd::NetworkGone => println!("ended network_gone"),
                    }
                    session.stop().await;
                    return Ok(());
                }
                println!(
                    "status devices={} refused={} tunnel={}",
                    status.devices, status.refused_sign_ins, status.tunnel
                );
            }
            stopped = stop_signal() => {
                stopped?;
                session.stop().await;
                println!("stopped");
                return Ok(());
            }
        }
    }
}

fn write_private(path: &PathBuf, text: &str) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(text.as_bytes())
}

#[cfg(unix)]
async fn stop_signal() -> Result<(), String> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut terminate =
        signal(SignalKind::terminate()).map_err(|error| format!("signal: {error}"))?;
    tokio::select! {
        _ = terminate.recv() => Ok(()),
        interrupted = tokio::signal::ctrl_c() => interrupted.map_err(|error| format!("signal: {error}")),
    }
}

#[cfg(not(unix))]
async fn stop_signal() -> Result<(), String> {
    tokio::signal::ctrl_c()
        .await
        .map_err(|error| format!("signal: {error}"))
}
