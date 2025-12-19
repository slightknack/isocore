//! # Host component instances
//!
//! Exhaustive enum of all host components supported by the runtime.
//! Each variant provides native host functionality to Wasm components.

use wasmtime::component::Linker;

use crate::context::ContextBuilder;
use crate::context::ExorunCtx;
use crate::host::Result;
use crate::host::Wasi;
use crate::host::Logger;
use crate::host::Kv;

/// Exhaustive enum of all system components supported by the runtime.
///
/// Each variant corresponds to a specific system component implementation
/// that provides host functionality to Wasm guests.
#[derive(Clone, Debug)]
pub enum HostInstance {
    /// Standard WASI (WebAssembly System Interface) functionality.
    /// Provides filesystem, stdio, environment variables, etc.
    Wasi(Wasi),
    /// Logger system component for capturing log messages.
    /// Provides the `exorun:host/logging` interface.
    Logger(Logger),
    /// Key-Value store system component for in-memory storage.
    /// Provides the `exorun:host/kv` interface.
    Kv(Kv),
}

impl HostInstance {
    /// Validates that this host instance can provide the specified interface.
    ///
    /// For WASI, any interface starting with "wasi:" is accepted since WASI
    /// provides many interfaces via a single `add_to_linker` call.
    /// For other host instances, the interface must match exactly.
    pub fn validate_interface(&self, interface: &str) -> Result<()> {
        let (name, expected) = match self {
            HostInstance::Wasi(_) if interface.starts_with("wasi:") => return Ok(()),
            HostInstance::Wasi(_) => ("WASI", "wasi:*"),
            HostInstance::Logger(_) if interface == "exorun:host/logging" => return Ok(()),
            HostInstance::Logger(_) => ("Logger", "exorun:host/logging"),
            HostInstance::Kv(_) if interface == "exorun:host/kv" => return Ok(()),
            HostInstance::Kv(_) => ("Kv", "exorun:host/kv"),
        };

        Err(crate::host::Error::Link(format!(
            "{} host instance cannot provide interface '{}' (expected '{}')",
            name, interface, expected
        )))
    }

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
            HostInstance::Logger(logger) => logger.link(linker),
            HostInstance::Kv(kv) => kv.link(linker),
        }
    }
}
