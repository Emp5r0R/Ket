use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    net::SocketAddr,
    path::Path,
};

use hmac::{Hmac, KeyInit, Mac};
use ket_core::SessionTransport;
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::Sha256;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zeroize::{Zeroize, Zeroizing};

pub const BROKER_PROTOCOL_VERSION: u16 = 1;
pub const DEFAULT_BROKER_ADDRESS: &str = "127.0.0.1:39731";
pub const MAX_FRAME_BYTES: usize = 128 * 1024;
pub const TOKEN_BYTES: usize = 32;

const PROOF_CONTEXT: &[u8] = b"ket-tunnel-broker-v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HandshakeChallenge {
    pub version: u16,
    pub nonce: [u8; 32],
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct HandshakeProof {
    pub mac: [u8; 32],
}

impl std::fmt::Debug for HandshakeProof {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HandshakeProof")
            .field("mac", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HandshakeResult {
    pub accepted: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "command")]
pub enum BrokerRequest {
    Ping,
    Probe {
        transport: SessionTransport,
    },
    Connect {
        transport: SessionTransport,
        resolved_addresses: Vec<SocketAddr>,
    },
    Status {
        tunnel_id: String,
    },
    Heartbeat {
        tunnel_id: String,
    },
    Stop {
        tunnel_id: String,
    },
}

impl std::fmt::Debug for BrokerRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ping => formatter.write_str("Ping"),
            Self::Probe { transport } => formatter
                .debug_struct("Probe")
                .field("transport", transport)
                .finish(),
            Self::Connect {
                transport,
                resolved_addresses,
            } => formatter
                .debug_struct("Connect")
                .field("transport", transport)
                .field("resolved_addresses", resolved_addresses)
                .finish(),
            Self::Status { .. } => formatter.write_str("Status { tunnel_id: [REDACTED] }"),
            Self::Heartbeat { .. } => formatter.write_str("Heartbeat { tunnel_id: [REDACTED] }"),
            Self::Stop { .. } => formatter.write_str("Stop { tunnel_id: [REDACTED] }"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "message")]
pub enum BrokerTunnelStatus {
    Connected,
    Stopped,
    Failed(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BrokerFault {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum BrokerResponse {
    Pong {
        engine_available: bool,
    },
    Probe {
        resolved_addresses: Vec<SocketAddr>,
        elapsed_ms: u64,
    },
    Connected {
        tunnel_id: String,
        handshake_latency_ms: u64,
    },
    Tunnel {
        status: BrokerTunnelStatus,
    },
    Stopped,
    Error {
        fault: BrokerFault,
    },
}

impl std::fmt::Debug for BrokerResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pong { engine_available } => formatter
                .debug_struct("Pong")
                .field("engine_available", engine_available)
                .finish(),
            Self::Probe {
                resolved_addresses,
                elapsed_ms,
            } => formatter
                .debug_struct("Probe")
                .field("resolved_addresses", resolved_addresses)
                .field("elapsed_ms", elapsed_ms)
                .finish(),
            Self::Connected {
                handshake_latency_ms,
                ..
            } => formatter
                .debug_struct("Connected")
                .field("tunnel_id", &"[REDACTED]")
                .field("handshake_latency_ms", handshake_latency_ms)
                .finish(),
            Self::Tunnel { status } => formatter
                .debug_struct("Tunnel")
                .field("status", status)
                .finish(),
            Self::Stopped => formatter.write_str("Stopped"),
            Self::Error { fault } => formatter.debug_tuple("Error").field(fault).finish(),
        }
    }
}

pub struct BrokerToken(Zeroizing<[u8; TOKEN_BYTES]>);

impl BrokerToken {
    pub fn generate() -> Self {
        let mut bytes = [0_u8; TOKEN_BYTES];
        OsRng.fill_bytes(&mut bytes);
        Self(Zeroizing::new(bytes))
    }

    pub fn load(path: &Path) -> Result<Self, TokenError> {
        let file = open_token(path)?;
        validate_permissions(&file)?;
        let mut bytes = Zeroizing::new(Vec::with_capacity(TOKEN_BYTES + 1));
        file.take((TOKEN_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() != TOKEN_BYTES {
            return Err(TokenError::InvalidLength);
        }
        let mut token = [0_u8; TOKEN_BYTES];
        token.copy_from_slice(bytes.as_slice());
        Ok(Self(Zeroizing::new(token)))
    }

    pub fn write_new(&self, path: &Path) -> Result<(), TokenError> {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o640);
        }
        let mut file = options.open(path)?;
        file.write_all(self.expose())?;
        file.sync_all()?;
        Ok(())
    }

    pub fn prove(&self, nonce: &[u8; 32]) -> HandshakeProof {
        HandshakeProof {
            mac: proof(self.expose(), nonce),
        }
    }

    pub fn verify(&self, nonce: &[u8; 32], candidate: &HandshakeProof) -> bool {
        verify_proof(self.expose(), nonce, &candidate.mac)
    }

    fn expose(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl std::fmt::Debug for BrokerToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BrokerToken([REDACTED])")
    }
}

#[derive(Debug, Error)]
pub enum TokenError {
    #[error("broker token file could not be read: {0}")]
    Io(#[from] std::io::Error),
    #[error("broker token must contain exactly 32 random bytes")]
    InvalidLength,
    #[error("broker token permissions allow access by untrusted local users")]
    InsecurePermissions,
}

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("broker message is larger than the protocol limit")]
    TooLarge,
    #[error("broker connection failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("broker message is invalid")]
    Invalid,
}

pub fn challenge() -> HandshakeChallenge {
    let mut nonce = [0_u8; 32];
    OsRng.fill_bytes(&mut nonce);
    HandshakeChallenge {
        version: BROKER_PROTOCOL_VERSION,
        nonce,
    }
}

pub async fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let mut encoded = serde_json::to_vec(value).map_err(|_| FrameError::Invalid)?;
    if encoded.len() > MAX_FRAME_BYTES {
        encoded.zeroize();
        return Err(FrameError::TooLarge);
    }
    let length = (encoded.len() as u32).to_be_bytes();
    let result = async {
        writer.write_all(&length).await?;
        writer.write_all(&encoded).await?;
        writer.flush().await?;
        Ok::<_, std::io::Error>(())
    }
    .await;
    encoded.zeroize();
    result.map_err(FrameError::Io)
}

pub async fn read_frame<R, T>(reader: &mut R) -> Result<T, FrameError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length).await?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge);
    }
    let mut encoded = vec![0_u8; length];
    reader.read_exact(&mut encoded).await?;
    let result = serde_json::from_slice(&encoded).map_err(|_| FrameError::Invalid);
    encoded.zeroize();
    result
}

fn proof(token: &[u8], nonce: &[u8; 32]) -> [u8; 32] {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(token).expect("the broker token length is accepted by HMAC");
    mac.update(PROOF_CONTEXT);
    mac.update(nonce);
    let output = mac.finalize().into_bytes();
    let mut proof = [0_u8; 32];
    proof.copy_from_slice(&output);
    proof
}

fn verify_proof(token: &[u8], nonce: &[u8; 32], candidate: &[u8]) -> bool {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(token).expect("the broker token length is accepted by HMAC");
    mac.update(PROOF_CONTEXT);
    mac.update(nonce);
    mac.verify_slice(candidate).is_ok()
}

fn open_token(path: &Path) -> Result<File, std::io::Error> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    options.open(path)
}

#[cfg(unix)]
fn validate_permissions(file: &File) -> Result<(), TokenError> {
    use std::os::unix::fs::PermissionsExt;

    let mode = file.metadata()?.permissions().mode();
    if mode & 0o037 != 0 {
        Err(TokenError::InsecurePermissions)
    } else {
        Ok(())
    }
}

#[cfg(not(unix))]
fn validate_permissions(_file: &File) -> Result<(), TokenError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::fs;

    #[cfg(unix)]
    use rand::{Rng, distributions::Alphanumeric};

    use super::*;

    #[cfg(unix)]
    fn temporary_token_path(label: &str) -> std::path::PathBuf {
        let suffix: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        std::env::temp_dir().join(format!("ket-{label}-{suffix}"))
    }

    #[test]
    fn challenge_proofs_verify_without_exposing_the_token() {
        let token = BrokerToken::generate();
        let challenge = challenge();
        let valid = token.prove(&challenge.nonce);
        assert!(token.verify(&challenge.nonce, &valid));

        let other = BrokerToken::generate();
        assert!(!other.verify(&challenge.nonce, &valid));
        assert_eq!(format!("{token:?}"), "BrokerToken([REDACTED])");
    }

    #[tokio::test]
    async fn framing_round_trips_and_rejects_oversized_lengths() {
        let (mut client, mut server) = tokio::io::duplex(1024);
        let writer =
            tokio::spawn(async move { write_frame(&mut client, &BrokerRequest::Ping).await });
        let request: BrokerRequest = read_frame(&mut server).await.unwrap();
        assert!(matches!(request, BrokerRequest::Ping));
        writer.await.unwrap().unwrap();

        let (mut client, mut server) = tokio::io::duplex(16);
        client
            .write_all(&((MAX_FRAME_BYTES as u32) + 1).to_be_bytes())
            .await
            .unwrap();
        assert!(matches!(
            read_frame::<_, BrokerRequest>(&mut server).await,
            Err(FrameError::TooLarge)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn token_reader_rejects_permissive_modes_and_symbolic_links() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let token_path = temporary_token_path("token-permissions");
        let link_path = temporary_token_path("token-symlink");
        BrokerToken::generate().write_new(&token_path).unwrap();
        fs::set_permissions(&token_path, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(matches!(
            BrokerToken::load(&token_path),
            Err(TokenError::InsecurePermissions)
        ));

        fs::set_permissions(&token_path, fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&token_path, &link_path).unwrap();
        assert!(matches!(
            BrokerToken::load(&link_path),
            Err(TokenError::Io(_))
        ));

        fs::remove_file(link_path).unwrap();
        fs::remove_file(token_path).unwrap();
    }
}
