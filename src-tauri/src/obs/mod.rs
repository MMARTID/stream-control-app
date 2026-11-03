//! Adaptador obs-websocket v5.

pub mod api;
pub mod auth;
pub mod client;
pub mod protocol;

pub use client::{ConnectionInfo, ConnectionState, ObsClient, ObsConfig, ObsError};
