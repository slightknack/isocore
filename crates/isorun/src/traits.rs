//! Core trait definitions for extensibility.
//!
//! These traits define the interfaces that users implement to extend isorun:
//!
//! - **Transport**: How to move RPC bytes (TCP, QUIC, WebSocket, etc.)
//! - **SystemComponent**: How to provide native Rust implementations of WIT interfaces

use anyhow::Result;
use wasmtime::component::Linker;

use crate::context::ContextBuilder;
use crate::context::IsorunCtx;

/// A transport for moving RPC bytes between runtimes.
///
/// Implement this trait to add support for new network protocols or IPC mechanisms.
///
/// # Examples
///
/// - TCP sockets
/// - QUIC connections
/// - WebSocket
/// - Unix domain sockets
/// - In-memory channels (for testing)
///
/// # Protocol
///
/// The `payload` is a complete neorpc frame (Call or Reply). The transport
/// doesn't need to understand the structure - just move the bytes reliably.
#[async_trait::async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Send an RPC payload and await the response.
    ///
    /// This should be a blocking call that waits for the response before returning.
    ///
    /// # Errors
    ///
    /// Return an error if the transport fails (connection lost, timeout, etc.).
    async fn call(&self, payload: &[u8]) -> Result<Vec<u8>>;
}

/// A native Rust implementation of a WIT interface.
///
/// System components provide the fastest path for host functionality - no
/// serialization overhead, direct Rust function calls.
///
/// # Lifecycle
///
/// 1. **Install**: Add function definitions to the Wasmtime linker
/// 2. **Configure**: Set up instance state (WASI preopens, env vars, etc.)
///
/// # Example
///
/// ```rust,no_run
/// use isorun::{SystemComponent, IsorunCtx, ContextBuilder};
/// use wasmtime::component::Linker;
/// use anyhow::Result;
///
/// struct MyFilesystem;
///
/// impl SystemComponent for MyFilesystem {
///     fn install(&self, linker: &mut Linker<IsorunCtx>) -> Result<()> {
///         // Bind host functions here
///         Ok(())
///     }
///
///     fn configure(&self, builder: &mut ContextBuilder) -> Result<()> {
///         // Add WASI preopens, etc.
///         Ok(())
///     }
/// }
/// ```
pub trait SystemComponent: Send + Sync + 'static {
    /// Install host function definitions into the linker.
    ///
    /// This is called during the linking phase, before instantiation.
    fn install(&self, linker: &mut Linker<IsorunCtx>) -> Result<()>;

    /// Configure the instance's initial state.
    ///
    /// This is called after `install()` but before instantiation. Use it to
    /// set up WASI preopens, environment variables, or inject custom data.
    fn configure(&self, _builder: &mut ContextBuilder) -> Result<()> {
        Ok(())
    }
}
