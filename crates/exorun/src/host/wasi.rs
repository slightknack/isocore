//! # WASI host component default implementation
//!
//! Provides standard WASI (WebAssembly System Interface) functionality to Wasm components.
//! This includes filesystem access, stdio, environment variables, and other OS-level capabilities.

use wasmtime::component::Linker;

use crate::context::ExorunCtx;
use crate::host::Result;

/// WASI system component that provides standard WASI functionality.
///
/// This component links the WASI interfaces (filesystem, stdio, etc.) to the guest.
/// Configuration is handled through the ContextBuilder's WASI methods.
#[derive(Clone, Debug, Default)]
pub struct Wasi;

impl Wasi {
    pub fn new() -> Self {
        Self
    }

    /// Links WASI to the linker and context builder.
    ///
    /// This installs WASI interfaces into the linker. WASI configuration
    /// is done via ContextBuilder methods directly (e.g., inheriting stdio,
    /// mounting directories), so the context_builder parameter is unused here.
    pub fn link(
        &self,
        linker: &mut Linker<ExorunCtx>,
    ) -> Result<()> {
        wasmtime_wasi::p2::add_to_linker_async(linker)?;
        Ok(())
    }
}
