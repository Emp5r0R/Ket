use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const SYSTEM_RESOLVER_PATH: &str = "/etc/resolv.conf";
const VIRTUAL_DNS_CONFIGURATION: &[u8] = b"nameserver 198.18.0.1\n";
const OPENVPN_DNS_CONFIGURATION: &[u8] =
    b"nameserver 1.1.1.1\nnameserver 1.0.0.1\noptions timeout:2 attempts:2\n";
const MAX_RESOLVER_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct SystemDnsManager {
    state_path: PathBuf,
    configuration: &'static [u8],
}

impl SystemDnsManager {
    pub(crate) fn virtual_dns(state_path: impl Into<PathBuf>) -> Self {
        Self {
            state_path: state_path.into(),
            configuration: VIRTUAL_DNS_CONFIGURATION,
        }
    }

    pub(crate) fn openvpn(state_path: impl Into<PathBuf>) -> Self {
        Self {
            state_path: state_path.into(),
            configuration: OPENVPN_DNS_CONFIGURATION,
        }
    }

    pub(crate) fn acquire(&self) -> io::Result<SystemDnsLease> {
        #[cfg(target_os = "linux")]
        {
            if unsafe { libc::geteuid() } == 0 {
                return SystemDnsLease::acquire_at(
                    Path::new(SYSTEM_RESOLVER_PATH),
                    &self.state_path,
                    self.configuration,
                );
            }
        }
        Ok(SystemDnsLease::inactive())
    }
}

#[derive(Debug)]
pub(crate) struct SystemDnsLease {
    resolver_path: PathBuf,
    state_path: PathBuf,
    active: bool,
}

impl SystemDnsLease {
    fn inactive() -> Self {
        Self {
            resolver_path: PathBuf::new(),
            state_path: PathBuf::new(),
            active: false,
        }
    }

    fn acquire_at(
        resolver_path: &Path,
        state_path: &Path,
        configuration: &'static [u8],
    ) -> io::Result<Self> {
        recover_at(resolver_path, state_path)?;
        let original = read_bounded(resolver_path)?;
        persist_state(state_path, &original)?;
        if let Err(error) = write_in_place(resolver_path, configuration) {
            let _ = write_in_place(resolver_path, &original);
            let _ = remove_state(state_path);
            return Err(error);
        }
        Ok(Self {
            resolver_path: resolver_path.to_owned(),
            state_path: state_path.to_owned(),
            active: true,
        })
    }
}

impl Drop for SystemDnsLease {
    fn drop(&mut self) {
        if self.active {
            let _ = recover_at(&self.resolver_path, &self.state_path);
            self.active = false;
        }
    }
}

pub fn recover_system_dns(state_path: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        recover_at(Path::new(SYSTEM_RESOLVER_PATH), state_path)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = state_path;
        Ok(())
    }
}

fn recover_at(resolver_path: &Path, state_path: &Path) -> io::Result<()> {
    let original = match read_optional_state(state_path)? {
        Some(original) => original,
        None => return Ok(()),
    };
    let current = read_bounded(resolver_path)?;
    if [VIRTUAL_DNS_CONFIGURATION, OPENVPN_DNS_CONFIGURATION].contains(&current.as_slice()) {
        write_in_place(resolver_path, &original)?;
    }
    remove_state(state_path)
}

fn read_optional_state(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() || metadata.len() > MAX_RESOLVER_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the saved resolver state is invalid",
                ));
            }
            read_bounded(path).map(Some)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_RESOLVER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the resolver configuration is invalid",
        ));
    }
    let mut content = Vec::with_capacity(metadata.len() as usize);
    File::open(path)?
        .take(MAX_RESOLVER_BYTES + 1)
        .read_to_end(&mut content)?;
    if content.len() as u64 > MAX_RESOLVER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the resolver configuration is too large",
        ));
    }
    Ok(content)
}

fn persist_state(path: &Path, content: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the resolver state path has no parent",
        )
    })?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    let temporary = path.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(target_os = "linux")]
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(&temporary)?;
    file.write_all(content)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    sync_parent(parent)
}

fn write_in_place(path: &Path, content: &[u8]) -> io::Result<()> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the resolver path is not a regular file",
        ));
    }
    let mut file = OpenOptions::new().write(true).truncate(true).open(path)?;
    file.write_all(content)?;
    file.sync_all()
}

fn remove_state(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => path.parent().map_or(Ok(()), sync_parent),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn sync_parent(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "ket-system-dns-{label}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn lease_applies_virtual_dns_and_restores_original_configuration() {
        let directory = TestDir::new("restore");
        let resolver = directory.path().join("resolv.conf");
        let state = directory.path().join("state/resolver");
        let original = b"nameserver 192.0.2.53\nsearch example.test\n";
        fs::write(&resolver, original).unwrap();

        {
            let _lease =
                SystemDnsLease::acquire_at(&resolver, &state, VIRTUAL_DNS_CONFIGURATION).unwrap();
            assert_eq!(fs::read(&resolver).unwrap(), VIRTUAL_DNS_CONFIGURATION);
            assert_eq!(fs::read(&state).unwrap(), original);
            #[cfg(unix)]
            assert_eq!(
                fs::metadata(&state).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        assert_eq!(fs::read(&resolver).unwrap(), original);
        assert!(!state.exists());
    }

    #[test]
    fn startup_recovery_repairs_a_stale_virtual_resolver() {
        let directory = TestDir::new("stale");
        let resolver = directory.path().join("resolv.conf");
        let state = directory.path().join("resolver.state");
        let original = b"nameserver 203.0.113.53\n";
        fs::write(&resolver, VIRTUAL_DNS_CONFIGURATION).unwrap();
        fs::write(&state, original).unwrap();

        recover_at(&resolver, &state).unwrap();

        assert_eq!(fs::read(&resolver).unwrap(), original);
        assert!(!state.exists());
    }

    #[test]
    fn openvpn_lease_uses_tunnel_routed_public_resolvers() {
        let directory = TestDir::new("openvpn");
        let resolver = directory.path().join("resolv.conf");
        let state = directory.path().join("resolver.state");
        let original = b"nameserver 192.0.2.53\n";
        fs::write(&resolver, original).unwrap();

        {
            let _lease =
                SystemDnsLease::acquire_at(&resolver, &state, OPENVPN_DNS_CONFIGURATION).unwrap();
            assert_eq!(fs::read(&resolver).unwrap(), OPENVPN_DNS_CONFIGURATION);
        }

        assert_eq!(fs::read(&resolver).unwrap(), original);
        assert!(!state.exists());
    }

    #[test]
    fn recovery_does_not_overwrite_a_new_network_configuration() {
        let directory = TestDir::new("network-change");
        let resolver = directory.path().join("resolv.conf");
        let state = directory.path().join("resolver.state");
        fs::write(&resolver, b"nameserver 192.0.2.53\n").unwrap();
        let lease =
            SystemDnsLease::acquire_at(&resolver, &state, VIRTUAL_DNS_CONFIGURATION).unwrap();
        let changed = b"nameserver 198.51.100.53\n";
        fs::write(&resolver, changed).unwrap();

        drop(lease);

        assert_eq!(fs::read(&resolver).unwrap(), changed);
        assert!(!state.exists());
    }
}
