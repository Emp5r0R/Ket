//! Shared Ket client orchestration for desktop and platform adapters.

mod api;
mod broker;
mod client;
mod error;
mod full_route;
mod hysteria;
mod invite;
mod leak_guard;
mod openvpn;
mod route_health;
mod runtime;
mod sealed_control;
mod shadowsocks;
mod state;
mod strategy;
mod system_dns;
mod transport;
mod validation;
mod wireguard;
mod xray;

pub use api::{ControlEndpoint, ControlPlane, HttpControlPlane, InsecureHttpPolicy, Sealing};
pub use broker::{BrokerConfig, BrokerReadiness, BrokerTransportAdapter};
pub use client::{KetClient, MaintenanceTask};
pub use error::{ClientError, ClientIssue};
pub use hysteria::Hysteria2Adapter;
pub use invite::InviteEnrollment;
pub use ket_tunnel_protocol::ConnectOptions;
pub use openvpn::OpenVpnStunnelAdapter;
pub use shadowsocks::Shadowsocks2022Adapter;
pub use state::{ClientPhase, ClientSnapshot, TransportFallback, TransportSummary};
pub use strategy::{SelectionPolicy, TransportHistory, TransportSelector};
pub use system_dns::recover_system_dns;
pub use transport::{ActiveTunnel, ProbeReport, StartedTunnel, TransportAdapter, TunnelStatus};
pub use wireguard::WireGuardTlsAdapter;
pub use xray::XrayAdapter;
