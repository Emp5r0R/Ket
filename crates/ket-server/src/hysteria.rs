use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::config::{HysteriaConfig, HysteriaObfuscation};

pub async fn write_runtime_config(config: &HysteriaConfig) -> Result<()> {
    let path = config.runtime_config_path.clone();
    let document = render_runtime_config(config);
    tokio::task::spawn_blocking(move || write_atomic(&path, &document))
        .await
        .context("Hysteria configuration writer stopped unexpectedly")??;
    Ok(())
}

fn render_runtime_config(config: &HysteriaConfig) -> Value {
    let mut document = json!({
        "listen": config.listen,
        "tls": {
            "cert": config.tls_cert_path,
            "key": config.tls_key_path,
            "sniGuard": "strict"
        },
        "auth": {
            "type": "http",
            "http": {
                "url": config.auth_url,
                "insecure": false
            }
        },
        "trafficStats": {
            "listen": ":9999",
            "secret": config.stats_secret
        },
        "disableUDP": false,
        "speedTest": false,
        "acl": {
            "inline": [
                "reject(0.0.0.0/8)",
                "reject(10.0.0.0/8)",
                "reject(100.64.0.0/10)",
                "reject(127.0.0.0/8)",
                "reject(169.254.0.0/16)",
                "reject(172.16.0.0/12)",
                "reject(192.168.0.0/16)",
                "reject(198.18.0.0/15)",
                "reject(224.0.0.0/4)",
                "reject(::1/128)",
                "reject(fc00::/7)",
                "reject(fe80::/10)",
                "reject(ff00::/8)",
                "reject(all, tcp/25)",
                "reject(all, tcp/465)",
                "reject(all, tcp/587)",
                "direct(all)"
            ]
        },
        "masquerade": {
            "type": "proxy",
            "proxy": {
                "url": config.masquerade_url,
                "rewriteHost": true,
                "insecure": false,
                "xForwarded": false
            }
        }
    });
    let obfs = match &config.obfuscation {
        HysteriaObfuscation::Disabled => None,
        HysteriaObfuscation::Salamander { password } => Some(json!({
            "type": "salamander",
            "salamander": { "password": password }
        })),
        HysteriaObfuscation::Gecko { password } => Some(json!({
            "type": "gecko",
            "gecko": {
                "password": password,
                "minPacketSize": 512,
                "maxPacketSize": 1200
            }
        })),
    };
    if let Some(obfs) = obfs {
        document["obfs"] = obfs;
    }
    document
}

fn write_atomic(path: &Path, document: &Value) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let temporary_path = path.with_extension("json.tmp");
    let encoded = serde_json::to_vec_pretty(document)?;

    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary_path)
        .with_context(|| format!("failed to open {}", temporary_path.display()))?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    fs::rename(&temporary_path, path)?;
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_uses_http_auth_stats_and_abuse_guards() {
        let document = render_runtime_config(&test_config());

        assert_eq!(document["auth"]["type"], "http");
        assert_eq!(
            document["auth"]["http"]["url"],
            "http://control-plane:8787/internal/v1/hysteria2/auth"
        );
        assert_eq!(document["obfs"]["type"], "salamander");
        assert_eq!(document["tls"]["sniGuard"], "strict");
        let acl = document["acl"]["inline"]
            .as_array()
            .expect("ACL must be an array");
        assert!(acl.contains(&json!("reject(169.254.0.0/16)")));
        assert!(acl.contains(&json!("reject(198.18.0.0/15)")));
        assert!(acl.contains(&json!("reject(all, tcp/25)")));
        assert_eq!(acl.last(), Some(&json!("direct(all)")));
    }

    fn test_config() -> HysteriaConfig {
        HysteriaConfig {
            transport_id: "hy2-primary".to_owned(),
            runtime_config_path: "/tmp/hysteria.json".into(),
            listen: ":8443".to_owned(),
            public_host: "vpn.example.test".to_owned(),
            public_port: 443,
            sni: "vpn.example.test".to_owned(),
            tls_cert_path: "/tls/fullchain.pem".to_owned(),
            tls_key_path: "/tls/privkey.pem".to_owned(),
            auth_url: "http://control-plane:8787/internal/v1/hysteria2/auth".to_owned(),
            stats_url: "http://hysteria2:9999".to_owned(),
            stats_secret: "stats-secret-at-least-thirty-two-characters".to_owned(),
            masquerade_url: "https://example.com/".to_owned(),
            obfuscation: HysteriaObfuscation::Salamander {
                password: "obfuscation-secret-at-least-32-characters".to_owned(),
            },
        }
    }
}
