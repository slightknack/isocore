//! # System Target
//!
//! Exhaustive enum of all system components supported by the runtime.
//! Each variant provides native host functionality to Wasm components.

use wasmtime::component::Linker;

use crate::context::ContextBuilder;
use crate::context::ExorunCtx;
use crate::system::Result;
use crate::system::WasiSystem;

/// Exhaustive enum of all system components supported by the runtime.
///
/// Each variant corresponds to a specific system component implementation
/// that provides host functionality to Wasm guests.
#[derive(Clone, Debug)]
pub enum SystemTarget {
    /// Standard WASI (WebAssembly System Interface) functionality.
    /// Provides filesystem, stdio, environment variables, etc.
    Wasi(WasiSystem),
}

impl SystemTarget {
    /// Links this system component to the linker and context builder.
    ///
    /// This performs both installation (adding host functions to the linker)
    /// and configuration (provisioning resources to the context builder) in a
    /// single operation.
    pub fn link(&self, linker: &mut Linker<ExorunCtx>, context_builder: &mut ContextBuilder) -> Result<()> {
        match self {
            SystemTarget::Wasi(wasi) => wasi.link(linker, context_builder),
        }
    }

    /// Returns the WIT interface name that this system component provides.
    ///
    /// For WASI, this returns None since WASI interfaces are implicitly known
    /// and don't follow the standard interface naming convention.
    pub fn interface_name(&self) -> Option<&str> {
        match self {
            SystemTarget::Wasi(_) => None, // WASI is special - it's implicitly added
        }
    }
}
