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
mod route_keeper;
mod runtime;
mod sealed_control;
mod shadowsocks;
mod share;
mod state;
mod strategy;
mod system_dns;
mod transport;
mod transport_memory;
mod validation;
mod wireguard;
mod xray;

pub use api::{ControlEndpoint, ControlPlane, HttpControlPlane, InsecureHttpPolicy, Sealing};
pub use broker::{BrokerConfig, BrokerReadiness, BrokerTransportAdapter};
pub use client::{KetClient, MaintenanceTask};
pub use error::{ClientError, ClientIssue};
pub use hysteria::Hysteria2Adapter;
pub use invite::InviteEnrollment;
pub use ket_tunnel_protocol::{
    BypassList, ConnectOptions, IpPrefix, MAX_BYPASS_ENTRIES, NetworkReport, PrefixError,
};
pub use openvpn::OpenVpnStunnelAdapter;
pub use route_keeper::{NetworkKind, network_kind};
pub use shadowsocks::Shadowsocks2022Adapter;
pub use share::{
    PREFERRED_PORT as SHARE_PORT, ShareDetails, ShareEnd, ShareError, ShareNetwork, ShareSession,
    ShareStatus, TunnelEgress, share_networks,
};
pub use state::{ClientPhase, ClientSnapshot, FallbackStage, TransportFallback, TransportSummary};
pub use strategy::{SelectionPolicy, TransportHistory, TransportSelector};
pub use system_dns::recover_system_dns;
pub use transport::{ActiveTunnel, ProbeReport, StartedTunnel, TransportAdapter, TunnelStatus};
pub use transport_memory::{NetworkTransportMemory, TransportMemory};
pub use wireguard::WireGuardTlsAdapter;
pub use xray::XrayAdapter;
