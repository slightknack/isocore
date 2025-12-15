//! # System Component Interface
//!
//! Defines the contract for native Rust code that exposes functionality to Wasm components.
//! System components can install themselves into a linker (defining interfaces) and configure
//! context builders (provisioning resources).

use wasmtime::component::Linker;

use crate::context::ContextBuilder;
use crate::context::ExorunCtx;

#[derive(Debug)]
pub enum Error {
    Linker(String),
    Config(String),
    Wasmtime(wasmtime::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Linker(msg) => write!(f, "Linker error: {}", msg),
            Self::Config(msg) => write!(f, "Configuration error: {}", msg),
            Self::Wasmtime(e) => write!(f, "Wasmtime error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

impl From<wasmtime::Error> for Error {
    fn from(e: wasmtime::Error) -> Self {
        Self::Wasmtime(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Trait for system components that provide host functionality to Wasm guests.
///
/// System components represent the contract between native Rust code and Wasm interfaces.
/// They must be able to:
/// 1. Install their interface definitions into a linker (what functions are available)
/// 2. Configure context resources (what capabilities are provisioned)
pub trait SystemComponent: Send + Sync + 'static {
    /// Installs this component's interface into the linker.
    ///
    /// This defines what functions the Wasm guest can import from this system component.
    fn install(&self, linker: &mut Linker<ExorunCtx>) -> Result<()>;

    /// Configures the context builder with any required resources.
    ///
    /// This provisions capabilities like file descriptors, environment variables, etc.
    fn configure(&self, builder: &mut ContextBuilder) -> Result<()>;
}

/// WASI system component that provides standard WASI functionality.
///
/// This component links the WASI interfaces (filesystem, stdio, etc.) to the guest.
/// Configuration is handled through the ContextBuilder's WASI methods.
#[derive(Clone)]
pub struct WasiSystem;

impl WasiSystem {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WasiSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemComponent for WasiSystem {
    fn install(&self, linker: &mut Linker<ExorunCtx>) -> Result<()> {
        wasmtime_wasi::p2::add_to_linker_async(linker)?;
        Ok(())
    }

    fn configure(&self, _builder: &mut ContextBuilder) -> Result<()> {
        // WASI configuration is done via ContextBuilder methods directly.
        // This component just ensures the linker has WASI functions available.
        Ok(())
    }
}
