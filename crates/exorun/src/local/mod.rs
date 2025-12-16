//! # Local components on the same machine
//!
//! Defines system components - native Rust implementations that provide functionality
//! to Wasm components through host-defined interfaces.

pub mod instance;
pub mod builder;

pub use instance::LocalInstance;
pub(crate) use instance::State;
pub use builder::InstanceBuilder;
