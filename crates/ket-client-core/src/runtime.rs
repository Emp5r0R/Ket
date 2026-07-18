use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use rand::{Rng, distributions::Alphanumeric};
use zeroize::Zeroize;

pub(crate) struct EphemeralConfig {
    path: PathBuf,
}

impl EphemeralConfig {
    pub(crate) async fn create(
        runtime_dir: &Path,
        prefix: &str,
        mut document: Vec<u8>,
    ) -> Result<Self, String> {
        let runtime_dir = runtime_dir.to_owned();
        let prefix = prefix.to_owned();
        tokio::task::spawn_blocking(move || {
            let result = write_private_config(&runtime_dir, &prefix, &document);
            document.zeroize();
            result.map(|path| Self { path })
        })
        .await
        .map_err(|_| "configuration writer stopped unexpectedly".to_owned())?
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for EphemeralConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn write_private_config(
    runtime_dir: &Path,
    prefix: &str,
    document: &[u8],
) -> Result<PathBuf, String> {
    fs::create_dir_all(runtime_dir)
        .map_err(|_| "failed to create the private runtime directory".to_owned())?;
    #[cfg(unix)]
    fs::set_permissions(
        runtime_dir,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .map_err(|_| "failed to secure the private runtime directory".to_owned())?;
    let suffix: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .collect();
    let path = runtime_dir.join(format!("{prefix}-{suffix}.json"));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|_| "failed to create the private transport configuration".to_owned())?;
    if file.write_all(document).is_err() || file.sync_all().is_err() {
        let _ = fs::remove_file(&path);
        return Err("failed to write the private transport configuration".to_owned());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[tokio::test]
    async fn private_config_is_removed_on_drop() {
        let directory = std::env::temp_dir().join(format!(
            "ket-private-config-{}",
            rand::thread_rng().r#gen::<u64>()
        ));
        let config = EphemeralConfig::create(&directory, "transport", b"secret".to_vec())
            .await
            .expect("create private config");
        let path = config.path().to_owned();
        assert_eq!(fs::read(&path).expect("read config"), b"secret");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).expect("metadata").permissions().mode() & 0o777,
                0o600
            );
        }
        drop(config);
        assert!(!path.exists());
        let _ = fs::remove_dir(directory);
    }
}
