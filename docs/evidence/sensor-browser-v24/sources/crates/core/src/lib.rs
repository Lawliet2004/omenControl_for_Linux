//! `omen-core` — shared types, capability model, configuration and errors for
//! the omen-linux stack (daemon, CLI, GUI).
//!
//! Everything the UI displays and the daemon enforces flows through the types
//! in this crate so that "capability-gated, never-fake" behavior is a property
//! of the type system rather than of each call site.

pub mod capability;
pub mod compatibility;
pub mod config;
pub mod error;
pub mod fans;
pub mod games;
pub mod lighting;
pub mod machine;
pub mod network;
pub mod power;
pub mod profile;
pub mod sensors;
pub mod settings;

pub use capability::{Capabilities, Capability, ControlPlane};
pub use error::{OmenError, Result};
pub use machine::{MachineClass, MachineInfo};

/// Semantic version of the on-disk configuration schema.
pub const CONFIG_VERSION: u32 = 1;

/// Minimum safe manual fan duty in percent. Hardware writes are clamped to
/// this floor so a fan can never be fully stopped through omen-linux.
pub const MIN_FAN_DUTY_PCT: u8 = 20;
/// Maximum manual fan duty in percent.
pub const MAX_FAN_DUTY_PCT: u8 = 100;
