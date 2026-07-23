use anyhow::Result;
#[cfg(unix)]
use anyhow::bail;
#[cfg(not(windows))]
use ket_tunnel_service::restore_system_dns;
use ket_tunnel_service::{ServiceConfig, initialize_token, serve_until};
use tracing_subscriber::EnvFilter;

#[cfg(not(windows))]
#[tokio::main]
async fn main() -> Result<()> {
    initialize_logging();
    let config = ServiceConfig::from_env()?;
    if restore_dns_requested() {
        ensure_privileged()?;
        restore_system_dns(&config)?;
        println!("restored system DNS");
        return Ok(());
    }
    if init_token_requested() {
        ensure_privileged()?;
        initialize_token(&config.token_file)?;
        println!("initialized {}", config.token_file.display());
        return Ok(());
    }
    ensure_privileged()?;
    serve_until(config, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
}

#[cfg(windows)]
fn main() -> Result<()> {
    if init_token_requested() {
        let config = ServiceConfig::from_env()?;
        initialize_token(&config.token_file)?;
        println!("initialized {}", config.token_file.display());
        return Ok(());
    }
    if std::env::args().any(|argument| argument == "--console") {
        initialize_logging();
        return windows_service_host::run_console();
    }
    windows_service_host::run_dispatcher()
}

fn init_token_requested() -> bool {
    std::env::args().any(|argument| argument == "--init-token")
}

#[cfg(not(windows))]
fn restore_dns_requested() -> bool {
    std::env::args().any(|argument| argument == "--restore-dns")
}

fn initialize_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .compact()
        .init();
}

#[cfg(unix)]
fn ensure_privileged() -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        bail!("ket-tunnel-service must run as root or through the system service manager");
    }
    Ok(())
}

#[cfg(all(not(unix), not(windows)))]
fn ensure_privileged() -> Result<()> {
    Ok(())
}

#[cfg(windows)]
mod windows_service_host {
    use std::{
        ffi::OsString,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use anyhow::{Context, Result};
    use tokio::sync::oneshot;
    use windows_service::{
        define_windows_service,
        service::{
            ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
        service_dispatcher,
    };

    use super::{ServiceConfig, serve_until};

    const SERVICE_NAME: &str = "KetTunnel";
    const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

    define_windows_service!(ffi_service_main, service_main);

    pub fn run_dispatcher() -> Result<()> {
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)
            .context("failed to start the Windows service dispatcher")
    }

    pub fn run_console() -> Result<()> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("failed to create the tunnel service runtime")?;
        runtime.block_on(async {
            let config = ServiceConfig::from_env()?;
            serve_until(config, async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
        })
    }

    fn service_main(_arguments: Vec<OsString>) {
        if let Err(error) = run_service() {
            tracing::error!(%error, "Ket tunnel service stopped with an error");
        }
    }

    fn run_service() -> Result<()> {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let shutdown_tx = Arc::new(Mutex::new(Some(shutdown_tx)));
        let handler_tx = Arc::clone(&shutdown_tx);
        let event_handler = move |event| match event {
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            ServiceControl::Stop | ServiceControl::Shutdown => {
                if let Ok(mut sender) = handler_tx.lock() {
                    if let Some(sender) = sender.take() {
                        let _ = sender.send(());
                    }
                }
                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        };
        let status = service_control_handler::register(SERVICE_NAME, event_handler)
            .context("failed to register the Windows service control handler")?;
        status.set_service_status(service_status(ServiceState::StartPending, 1))?;

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("failed to create the tunnel service runtime")?;
        let config = ServiceConfig::from_env()?;
        status.set_service_status(service_status(ServiceState::Running, 0))?;
        let result = runtime.block_on(serve_until(config, async {
            let _ = shutdown_rx.await;
        }));
        let exit_code = if result.is_ok() { 0 } else { 1 };
        status.set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(exit_code),
            checkpoint: 0,
            wait_hint: Duration::ZERO,
            process_id: None,
        })?;
        result
    }

    fn service_status(state: ServiceState, checkpoint: u32) -> ServiceStatus {
        let running = state == ServiceState::Running;
        ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: state,
            controls_accepted: if running {
                ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
            } else {
                ServiceControlAccept::empty()
            },
            exit_code: ServiceExitCode::Win32(0),
            checkpoint,
            wait_hint: if running {
                Duration::ZERO
            } else {
                Duration::from_secs(10)
            },
            process_id: None,
        }
    }
}
