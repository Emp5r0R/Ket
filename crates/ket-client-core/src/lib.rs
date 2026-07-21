//! Shared Ket client orchestration for desktop and platform adapters.

mod api;
mod broker;
mod client;
mod error;
mod full_route;
mod hysteria;
mod runtime;
mod state;
mod strategy;
mod transport;
mod validation;
mod xray;

pub use api::{ControlEndpoint, ControlPlane, HttpControlPlane, InsecureHttpPolicy};
pub use broker::{BrokerConfig, BrokerReadiness, BrokerTransportAdapter};
pub use client::{KetClient, MaintenanceTask};
pub use error::{ClientError, ClientIssue};
pub use hysteria::Hysteria2Adapter;
pub use state::{ClientPhase, ClientSnapshot, TransportSummary};
pub use strategy::{SelectionPolicy, TransportHistory, TransportSelector};
pub use transport::{ActiveTunnel, ProbeReport, StartedTunnel, TransportAdapter, TunnelStatus};
pub use xray::XrayAdapter;
