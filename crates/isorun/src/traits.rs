//! Core trait definitions for transport and system components

use anyhow::Result;
use wasmtime::component::Linker;

use crate::context::ContextBuilder;
use crate::context::IsorunCtx;

/// The "One General Way" to move bytes.
/// Implement this for HTTP, TCP, QUIC, or specialized IPC.
#[async_trait::async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Send a serialized WIT payload and await the result.
    async fn call(&self, payload: &[u8]) -> Result<Vec<u8>>;
}

/// A local system implementation (e.g., Filesystem, Database).
pub trait SystemComponent: Send + Sync + 'static {
    /// Step 1: Install function definitions into the linker.
    fn install(&self, linker: &mut Linker<IsorunCtx>) -> Result<()>;

    /// Step 2: Configure the instance state (Preopens, Env Vars, Auth).
    fn configure(&self, _builder: &mut ContextBuilder) -> Result<()> {
        Ok(())
    }
}
