//! Shared Ket client orchestration for desktop and platform adapters.

mod api;
mod broker;
mod client;
mod error;
mod full_route;
mod hysteria;
mod openvpn;
mod runtime;
mod shadowsocks;
mod state;
mod strategy;
mod system_dns;
mod transport;
mod validation;
mod wireguard;
mod xray;

pub use api::{ControlEndpoint, ControlPlane, HttpControlPlane, InsecureHttpPolicy};
pub use broker::{BrokerConfig, BrokerReadiness, BrokerTransportAdapter};
pub use client::{KetClient, MaintenanceTask};
pub use error::{ClientError, ClientIssue};
pub use hysteria::Hysteria2Adapter;
pub use openvpn::OpenVpnStunnelAdapter;
pub use shadowsocks::Shadowsocks2022Adapter;
pub use state::{ClientPhase, ClientSnapshot, TransportSummary};
pub use strategy::{SelectionPolicy, TransportHistory, TransportSelector};
pub use system_dns::recover_system_dns;
pub use transport::{ActiveTunnel, ProbeReport, StartedTunnel, TransportAdapter, TunnelStatus};
pub use wireguard::WireGuardTlsAdapter;
pub use xray::XrayAdapter;
