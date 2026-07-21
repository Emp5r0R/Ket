use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, StatusCode, Uri, header};
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::TokioExecutor,
};
use ket_core::{SecretString, split_session_token};
use serde::Serialize;
use zeroize::{Zeroize, Zeroizing};

const MAX_CREDENTIAL_BYTES: u64 = 1024;

#[derive(Serialize)]
struct AuthRequest {
    username: String,
    password: SecretString,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("OpenVPN authentication failed: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .context("credential file argument is required")?;
    let metadata = tokio::fs::metadata(&path).await?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_CREDENTIAL_BYTES {
        bail!("credential file is invalid");
    }
    let mut contents = Zeroizing::new(tokio::fs::read_to_string(path).await?);
    if contents.contains('\0') || contents.contains('\r') {
        bail!("credentials contain invalid characters");
    }
    let mut lines = contents.lines();
    let username = lines.next().context("username is missing")?.to_owned();
    let password = lines.next().context("password is missing")?.to_owned();
    if lines.next().is_some() {
        bail!("credential file contains unexpected data");
    }
    let token = split_session_token(&password).context("password has an invalid shape")?;
    if token.id != username {
        bail!("username does not match the scoped token");
    }
    contents.zeroize();

    let url = Zeroizing::new(
        std::env::var("KET_OPENVPN_AUTH_URL").context("KET_OPENVPN_AUTH_URL is required")?,
    );
    let uri: Uri = url.parse().context("KET_OPENVPN_AUTH_URL is invalid")?;
    if uri.scheme_str() != Some("http") || uri.authority().is_none() {
        bail!("KET_OPENVPN_AUTH_URL must be an internal HTTP URL");
    }
    let auth_token = Zeroizing::new(
        std::env::var("KET_OPENVPN_AUTH_TOKEN").context("KET_OPENVPN_AUTH_TOKEN is required")?,
    );
    if auth_token.len() < 32 {
        bail!("KET_OPENVPN_AUTH_TOKEN is invalid");
    }
    let mut encoded = Zeroizing::new(serde_json::to_vec(&AuthRequest {
        username,
        password: password.into(),
    })?);
    let request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", auth_token.as_str()),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::copy_from_slice(&encoded)))?;
    encoded.zeroize();
    let mut connector = HttpConnector::new();
    connector.enforce_http(true);
    let client = Client::builder(TokioExecutor::new()).build(connector);
    let response = tokio::time::timeout(Duration::from_secs(5), client.request(request))
        .await
        .context("authentication request timed out")??;
    let status = response.status();
    let body = response.into_body().collect().await?.to_bytes();
    if body.len() > 4096 {
        bail!("authentication response is too large");
    }
    if status != StatusCode::NO_CONTENT {
        bail!("authentication was rejected");
    }
    Ok(())
}
