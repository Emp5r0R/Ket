//! Remembers which transports worked on each network, so Automatic starts
//! with the one that worked there last time: the campus network that only
//! lets REALITY through, the home network where Hysteria2 is fastest.
//!
//! Networks are told apart by a salted hash of the default route's interface,
//! gateway, and gateway hardware address. The file holds only those hashes,
//! a random salt, and transport IDs with their outcomes: no network name, no
//! address, and nothing about what was visited.

use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{TransportHistory, full_route::TUNNEL_INTERFACE, route_keeper};

/// The most networks remembered; the least recently seen goes first.
const MAX_NETWORKS: usize = 16;
const MAX_FILE_BYTES: u64 = 256 * 1024;

/// Where a client keeps its transport history between connections.
pub trait TransportMemory: Send + Sync {
    /// The history for the network the device is on now.
    fn load(&self) -> TransportHistory;
    fn save(&self, history: &TransportHistory);
}

#[derive(Default, Deserialize, Serialize)]
struct MemoryFile {
    salt: String,
    networks: BTreeMap<String, RememberedNetwork>,
}

#[derive(Deserialize, Serialize)]
struct RememberedNetwork {
    seen_at_epoch_seconds: u64,
    history: TransportHistory,
}

/// A [`TransportMemory`] in one JSON file, one history per network.
pub struct NetworkTransportMemory {
    path: PathBuf,
    network: Box<dyn Fn() -> Option<String> + Send + Sync>,
    lock: Mutex<()>,
}

impl NetworkTransportMemory {
    /// Keys histories by the physical network this computer is on.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self::with_network(path, || route_keeper::physical_network(TUNNEL_INTERFACE))
    }

    fn with_network(
        path: impl Into<PathBuf>,
        network: impl Fn() -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            path: path.into(),
            network: Box::new(network),
            lock: Mutex::new(()),
        }
    }

    fn read(&self) -> MemoryFile {
        read_file(&self.path).unwrap_or_default()
    }
}

impl TransportMemory for NetworkTransportMemory {
    fn load(&self) -> TransportHistory {
        let _guard = self.lock.lock();
        let Some(network) = (self.network)() else {
            return TransportHistory::default();
        };
        let file = self.read();
        file.networks
            .get(&network_key(&file.salt, &network))
            .map(|remembered| remembered.history.clone())
            .unwrap_or_default()
    }

    fn save(&self, history: &TransportHistory) {
        let _guard = self.lock.lock();
        let Some(network) = (self.network)() else {
            return;
        };
        let mut file = self.read();
        if file.salt.len() != 64 {
            let mut salt = [0_u8; 32];
            OsRng.fill_bytes(&mut salt);
            file.salt = hex(&salt);
            file.networks.clear();
        }
        let key = network_key(&file.salt, &network);
        file.networks.insert(
            key,
            RememberedNetwork {
                seen_at_epoch_seconds: unix_time(),
                history: history.clone(),
            },
        );
        while file.networks.len() > MAX_NETWORKS {
            let Some(oldest) = file
                .networks
                .iter()
                .min_by_key(|(_, remembered)| remembered.seen_at_epoch_seconds)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            file.networks.remove(&oldest);
        }
        if let Err(error) = write_file(&self.path, &file) {
            tracing::debug!(%error, "could not save the transport history");
        }
    }
}

fn network_key(salt: &str, network: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(salt.as_bytes());
    hash.update([0]);
    hash.update(network.as_bytes());
    hex(&hash.finalize()[..16])
}

fn read_file(path: &Path) -> Option<MemoryFile> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_FILE_BYTES {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn write_file(path: &Path, file: &MemoryFile) -> io::Result<()> {
    let encoded = serde_json::to_vec(file).map_err(io::Error::other)?;
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let staging = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    std::io::Write::write_all(&mut options.open(&staging)?, &encoded)?;
    std::fs::rename(staging, path)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    fn memory(path: &Path) -> (NetworkTransportMemory, Arc<Mutex<Option<String>>>) {
        let network = Arc::new(Mutex::new(Some("wlan0\n192.168.1.1\naa:bb".to_owned())));
        let current = Arc::clone(&network);
        (
            NetworkTransportMemory::with_network(path, move || current.lock().unwrap().clone()),
            network,
        )
    }

    fn temporary_path() -> PathBuf {
        std::env::temp_dir().join(format!("ket-memory-{}.json", OsRng.next_u64()))
    }

    #[test]
    fn each_network_keeps_its_own_history() {
        let path = temporary_path();
        let (memory, network) = memory(&path);
        let mut home = TransportHistory::default();
        home.record_success("hysteria", 20);
        memory.save(&home);

        *network.lock().unwrap() = Some("eth0\n10.0.0.1\ncc:dd".to_owned());
        assert_eq!(memory.load(), TransportHistory::default());
        let mut campus = TransportHistory::default();
        campus.record_success("reality", 90);
        memory.save(&campus);
        assert_eq!(memory.load(), campus);

        *network.lock().unwrap() = Some("wlan0\n192.168.1.1\naa:bb".to_owned());
        assert_eq!(memory.load(), home);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_file_names_no_network_and_survives_corruption() {
        let path = temporary_path();
        let (memory, network) = memory(&path);
        let mut history = TransportHistory::default();
        history.record_success("reality", 90);
        memory.save(&history);
        let text = std::fs::read_to_string(&path).unwrap();
        for secret in ["wlan0", "192.168.1.1", "aa:bb"] {
            assert!(!text.contains(secret), "{secret} reached the file");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(memory.load(), TransportHistory::default());
        memory.save(&history);
        assert_eq!(memory.load(), history);

        *network.lock().unwrap() = None;
        assert_eq!(
            memory.load(),
            TransportHistory::default(),
            "offline: nothing"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn only_the_most_recently_seen_networks_are_kept() {
        let path = temporary_path();
        let (memory, network) = memory(&path);
        let history = TransportHistory::default();
        for index in 0..(MAX_NETWORKS + 4) {
            *network.lock().unwrap() = Some(format!("network-{index}"));
            memory.save(&history);
        }
        let file = read_file(&path).unwrap();
        assert_eq!(file.networks.len(), MAX_NETWORKS);
        let _ = std::fs::remove_file(path);
    }
}
