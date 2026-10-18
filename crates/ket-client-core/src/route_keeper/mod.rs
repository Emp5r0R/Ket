//! Keeps the engines' direct routes to the Ket server on the current
//! physical network while a full-route tunnel is up (see [`plan`]).
//!
//! The keeper installs the routes before the bridge starts, checks the
//! routing table every [`REFRESH_INTERVAL`], and removes what it installed
//! when dropped, so every way a tunnel ends cleans up. Routes are tagged so a
//! later start removes any that a crashed process left behind.

#[cfg(target_os = "linux")]
#[cfg_attr(test, allow(dead_code))]
mod linux;
mod plan;
#[cfg(windows)]
#[cfg_attr(test, allow(dead_code))]
mod windows;

use std::{
    collections::BTreeMap,
    io,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{sync::watch, task::JoinHandle};

use crate::NetworkReport;

pub(crate) use plan::{DefaultRoute, Family, RouteAction, ServerPath};

/// How often the routing table is checked for a network change.
pub(crate) const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// The routing table, as the keeper needs it.
pub(crate) trait RouteSystem: Send + 'static {
    fn default_routes(&mut self) -> io::Result<Vec<DefaultRoute>>;
    fn apply(&mut self, action: &RouteAction) -> io::Result<()>;
    /// Removes routes a previous Ket process left behind.
    fn clear_stale(&mut self) -> io::Result<()>;
}

struct KeeperState {
    system: Box<dyn RouteSystem>,
    servers: Vec<IpAddr>,
    tunnel_interface: String,
    applied: BTreeMap<IpAddr, ServerPath>,
    report: watch::Sender<NetworkReport>,
}

impl KeeperState {
    fn refresh(&mut self) -> io::Result<()> {
        let routes = self.system.default_routes()?;
        let actions = plan::plan(
            &routes,
            &self.servers,
            &self.applied,
            &self.tunnel_interface,
        );
        let mut moved = false;
        for action in actions {
            self.system.apply(&action)?;
            if let RouteAction::Install {
                server,
                path,
                replacing,
            } = action
            {
                moved |= replacing.is_some();
                self.applied.insert(server, path);
            }
        }
        let offline = !self.applied.is_empty()
            && self
                .applied
                .values()
                .all(|path| matches!(path, ServerPath::Unreachable));
        self.report.send_if_modified(|report| {
            let next = NetworkReport {
                generation: report.generation + u64::from(moved),
                offline,
            };
            std::mem::replace(report, next) != next
        });
        Ok(())
    }
}

pub(crate) struct RouteKeeper {
    state: Arc<Mutex<KeeperState>>,
    task: Option<JoinHandle<()>>,
    report: watch::Receiver<NetworkReport>,
}

impl RouteKeeper {
    /// Installs a direct route for every server through the current physical
    /// network and keeps it there. Fails when the routes cannot be installed.
    pub(crate) fn engage(servers: &[IpAddr], tunnel_interface: &str) -> io::Result<Self> {
        Self::engage_with(
            platform_system(),
            servers,
            tunnel_interface,
            REFRESH_INTERVAL,
        )
    }

    fn engage_with(
        mut system: Box<dyn RouteSystem>,
        servers: &[IpAddr],
        tunnel_interface: &str,
        interval: Duration,
    ) -> io::Result<Self> {
        system.clear_stale()?;
        let (report, report_rx) = watch::channel(NetworkReport::default());
        let mut state = KeeperState {
            system,
            servers: servers.to_vec(),
            tunnel_interface: tunnel_interface.to_owned(),
            applied: BTreeMap::new(),
            report,
        };
        if let Err(error) = state.refresh() {
            release(&mut state);
            return Err(error);
        }
        let state = Arc::new(Mutex::new(state));
        let task = (!servers.is_empty()).then(|| {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    let refreshed = state.lock().map(|mut state| state.refresh());
                    if let Ok(Err(error)) = refreshed {
                        tracing::debug!(%error, "could not refresh the server routes");
                    }
                }
            })
        });
        Ok(Self {
            state,
            task,
            report: report_rx,
        })
    }

    /// Network changes and outages as the keeper sees them.
    pub(crate) fn network(&self) -> watch::Receiver<NetworkReport> {
        self.report.clone()
    }
}

impl Drop for RouteKeeper {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if let Ok(mut state) = self.state.lock() {
            release(&mut state);
        }
    }
}

fn release(state: &mut KeeperState) {
    for action in plan::release(&state.applied) {
        if let Err(error) = state.system.apply(&action) {
            tracing::debug!(%error, "could not remove a server route");
        }
    }
    state.applied.clear();
}

/// The physical network the device is on, as the default route's interface,
/// gateway, and (on Linux) the gateway's hardware address, so per-network
/// memory can tell networks apart. Callers hash it; it is never stored as is.
/// `None` without a physical network, or in unit tests.
pub(crate) fn physical_network(tunnel_interface: &str) -> Option<String> {
    let routes = physical_routes()?;
    let route = plan::best_default(&routes, Family::V4, tunnel_interface)
        .or_else(|| plan::best_default(&routes, Family::V6, tunnel_interface))?;
    let gateway = route
        .gateway
        .map(|gateway| gateway.to_string())
        .unwrap_or_default();
    let hardware = route.gateway.and_then(gateway_hardware).unwrap_or_default();
    Some(format!("{}\n{gateway}\n{hardware}", route.interface))
}

#[cfg(all(target_os = "linux", not(test)))]
fn physical_routes() -> Option<Vec<DefaultRoute>> {
    linux::read_default_routes().ok()
}

#[cfg(all(target_os = "linux", not(test)))]
fn gateway_hardware(gateway: IpAddr) -> Option<String> {
    linux::neighbour_hardware(gateway)
}

#[cfg(all(windows, not(test)))]
fn physical_routes() -> Option<Vec<DefaultRoute>> {
    windows::WindowsRoutes.default_routes().ok()
}

#[cfg(any(not(target_os = "linux"), test))]
fn gateway_hardware(_gateway: IpAddr) -> Option<String> {
    None
}

#[cfg(any(not(any(target_os = "linux", windows)), test))]
fn physical_routes() -> Option<Vec<DefaultRoute>> {
    None
}

#[cfg(all(target_os = "linux", not(test)))]
fn platform_system() -> Box<dyn RouteSystem> {
    Box::new(linux::LinuxRoutes::new())
}

#[cfg(all(windows, not(test)))]
fn platform_system() -> Box<dyn RouteSystem> {
    Box::new(windows::WindowsRoutes)
}

/// Elsewhere the bridge's own routes are used, and unit tests never touch
/// the real routing table.
#[cfg(any(not(any(target_os = "linux", windows)), test))]
fn platform_system() -> Box<dyn RouteSystem> {
    struct Unmanaged;
    impl RouteSystem for Unmanaged {
        fn default_routes(&mut self) -> io::Result<Vec<DefaultRoute>> {
            Ok(Vec::new())
        }
        fn apply(&mut self, _action: &RouteAction) -> io::Result<()> {
            Ok(())
        }
        fn clear_stale(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    Box::new(Unmanaged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Table {
        routes: Vec<DefaultRoute>,
        installed: BTreeMap<IpAddr, ServerPath>,
        log: Vec<String>,
        fail_installs: bool,
    }

    #[derive(Clone, Default)]
    struct FakeSystem(Arc<Mutex<Table>>);

    impl RouteSystem for FakeSystem {
        fn default_routes(&mut self) -> io::Result<Vec<DefaultRoute>> {
            Ok(self.0.lock().unwrap().routes.clone())
        }
        fn apply(&mut self, action: &RouteAction) -> io::Result<()> {
            let mut table = self.0.lock().unwrap();
            match action {
                RouteAction::Install { server, path, .. } => {
                    if table.fail_installs {
                        return Err(io::Error::other("refused"));
                    }
                    table.installed.insert(*server, path.clone());
                    table.log.push(format!("install {server}"));
                }
                RouteAction::Remove { server, .. } => {
                    table.installed.remove(server);
                    table.log.push(format!("remove {server}"));
                }
            }
            Ok(())
        }
        fn clear_stale(&mut self) -> io::Result<()> {
            self.0.lock().unwrap().log.push("clear".to_owned());
            Ok(())
        }
    }

    fn route(interface: &str, metric: u32) -> DefaultRoute {
        DefaultRoute {
            family: Family::V4,
            interface: interface.to_owned(),
            interface_index: metric,
            gateway: Some("192.0.2.1".parse().unwrap()),
            metric,
        }
    }

    #[tokio::test]
    async fn the_keeper_follows_network_changes_and_cleans_up_when_dropped() {
        let system = FakeSystem::default();
        system.0.lock().unwrap().routes = vec![route("wlan0", 600)];
        let server: IpAddr = "203.0.113.5".parse().unwrap();
        let keeper = RouteKeeper::engage_with(
            Box::new(system.clone()),
            &[server],
            "ket0",
            Duration::from_millis(20),
        )
        .unwrap();
        assert_eq!(
            system.0.lock().unwrap().installed.get(&server),
            Some(&ServerPath::Via(route("wlan0", 600)))
        );
        assert_eq!(*keeper.network().borrow(), NetworkReport::default());

        // Wi-Fi gives way to Ethernet; the next check moves the route.
        system.0.lock().unwrap().routes = vec![route("eth0", 100), route("ket0", 0)];
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            system.0.lock().unwrap().installed.get(&server),
            Some(&ServerPath::Via(route("eth0", 100)))
        );
        assert_eq!(
            *keeper.network().borrow(),
            NetworkReport {
                generation: 1,
                offline: false
            }
        );

        // Every network gone: the server is unreachable rather than looped.
        system.0.lock().unwrap().routes = vec![route("ket0", 0)];
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            system.0.lock().unwrap().installed.get(&server),
            Some(&ServerPath::Unreachable)
        );
        assert_eq!(
            *keeper.network().borrow(),
            NetworkReport {
                generation: 2,
                offline: true
            }
        );

        drop(keeper);
        let table = system.0.lock().unwrap();
        assert!(table.installed.is_empty());
        assert_eq!(table.log.first().map(String::as_str), Some("clear"));
        assert_eq!(
            table.log.last().map(String::as_str),
            Some("remove 203.0.113.5")
        );
    }

    #[tokio::test]
    async fn a_route_that_cannot_be_installed_fails_the_start() {
        let system = FakeSystem::default();
        {
            let mut table = system.0.lock().unwrap();
            table.routes = vec![route("wlan0", 600)];
            table.fail_installs = true;
        }
        let result = RouteKeeper::engage_with(
            Box::new(system.clone()),
            &["203.0.113.5".parse().unwrap()],
            "ket0",
            Duration::from_secs(2),
        );
        assert!(result.is_err());
        assert!(system.0.lock().unwrap().installed.is_empty());
    }
}
