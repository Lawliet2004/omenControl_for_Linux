//! Restricted hardware service and transport, shared with native integration tests.
pub mod coarse;
pub mod fan_guard;
mod game_sessions;
pub mod nvidia_reader;
pub mod server;
pub mod service;
pub mod thermal;

pub static TERMINATE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
