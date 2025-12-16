//! # Host component instances
//!
//! Exhaustive enum of all host components supported by the runtime.
//! Each variant provides native host functionality to Wasm components.

use wasmtime::component::Linker;

use crate::context::ContextBuilder;
use crate::context::ExorunCtx;
use crate::host::Result;
use crate::host::Wasi;

/// Exhaustive enum of all system components supported by the runtime.
///
/// Each variant corresponds to a specific system component implementation
/// that provides host functionality to Wasm guests.
#[derive(Clone, Debug)]
pub enum HostInstance {
    /// Standard WASI (WebAssembly System Interface) functionality.
    /// Provides filesystem, stdio, environment variables, etc.
    Wasi(Wasi),
}

impl HostInstance {
    /// Links this system component to the linker and context builder.
    ///
    /// This performs both installation (adding host functions to the linker)
    /// and configuration (provisioning resources to the context builder) in a
    /// single operation.
    pub fn link(
        &self,
        linker: &mut Linker<ExorunCtx>,
        _context_builder: &mut ContextBuilder
    ) -> Result<()> {
        match self {
            HostInstance::Wasi(wasi) => wasi.link(linker),
        }
    }

    /// Returns the WIT interface name that this system component provides.
    ///
    /// For WASI, this returns None since WASI interfaces are implicitly known
    /// and don't follow the standard interface naming convention.
    pub fn interface_name(&self) -> Option<&str> {
        match self {
            HostInstance::Wasi(_) => None, // WASI is special
        }
    }
}
