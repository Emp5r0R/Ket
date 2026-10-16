//! Release preflight probe: enrolls once from an invite read on stdin, using
//! the same client code the apps ship, then releases the session.
//!
//! Prints `key=value` lines (no secrets) for `scripts/release-preflight.sh`:
//! `node_signature=verified`, `server_version=…`, `client_update=…`,
//! `transports=…`, and `released=yes`. Exits non-zero on any failure.

use std::io::Read;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use ket_client_core::{ControlPlane, HttpControlPlane, InviteEnrollment};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("invite_enroll_check: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| format!("could not read stdin: {error}"))?;
    let invite = input
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("ket:invite:"))
        .ok_or("no ket:invite: line on stdin")?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs();
    let enrollment =
        InviteEnrollment::from_text(invite, now).map_err(|error| format!("invite: {error}"))?;
    let plane = HttpControlPlane::new().map_err(|error| format!("client: {error}"))?;

    // A node-keyed endpoint only accepts a manifest signed by that node.
    let manifest = plane
        .create_session(
            &enrollment.endpoint,
            &enrollment.access_code,
            "release preflight",
        )
        .await
        .map_err(|error| format!("enrollment: {error}"))?;
    println!("node_signature=verified");
    println!("node_fingerprint={}", enrollment.node_fingerprint);
    let mut protocols: Vec<String> = manifest
        .transports
        .iter()
        .map(|transport| format!("{:?}", transport.profile.protocol))
        .collect();
    protocols.sort();
    println!("transports={}", protocols.join(","));
    match &manifest.updates {
        Some(advice) => {
            println!("server_version={}", advice.server_version);
            println!("client_update={:?}", advice.client.status);
        }
        None => println!("server_version=unknown"),
    }

    let status = plane
        .session_status(&enrollment.endpoint, &manifest.session_token)
        .await;
    let released = plane
        .release_session(&enrollment.endpoint, &manifest.session_token)
        .await;
    status.map_err(|error| format!("session status: {error}"))?;
    released.map_err(|error| format!("release: {error}"))?;
    println!("released=yes");
    Ok(())
}
