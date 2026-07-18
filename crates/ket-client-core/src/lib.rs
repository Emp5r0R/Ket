//! Shared Ket client orchestration for desktop and platform adapters.

mod api;
mod broker;
mod client;
mod error;
mod hysteria;
mod state;
mod strategy;
mod transport;

pub use api::{ControlEndpoint, ControlPlane, HttpControlPlane, InsecureHttpPolicy};
pub use broker::{BrokerConfig, BrokerHysteriaAdapter, BrokerReadiness};
pub use client::{KetClient, MaintenanceTask};
pub use error::{ClientError, ClientIssue};
pub use hysteria::{Hysteria2Adapter, HysteriaTunSettings};
pub use state::{ClientPhase, ClientSnapshot, TransportSummary};
pub use strategy::{SelectionPolicy, TransportHistory, TransportSelector};
pub use transport::{ActiveTunnel, ProbeReport, StartedTunnel, TransportAdapter, TunnelStatus};
