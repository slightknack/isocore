//! Context types for instance configuration and execution.
//!
//! This module defines the request-scoped state that lives inside each
//! WebAssembly instance's Store.
//!
//! ## Flow
//!
//! 1. **ContextBuilder** - Staging area for configuration (WASI preopens, env vars)
//! 2. **IsorunCtx** - The actual Store context, built from a ContextBuilder
//!
//! System components can inject configuration during the linking phase,
//! and instances can access typed user data at runtime.

use wasmtime::component::ResourceTable;
use wasmtime_wasi::WasiCtxView;
use wasmtime_wasi::WasiView;

/// Resource limits for an instance.
///
/// Controls how much computation and memory an instance can consume.
#[derive(Clone, Debug)]
pub struct Budget {
    /// Maximum fuel units (roughly instructions) before trap.
    pub fuel: u64,
    /// Maximum memory bytes the instance can allocate.
    pub memory_bytes: u64,
}

impl Budget {
    /// Returns sensible defaults for general-purpose workloads.
    pub fn standard() -> Self {
        Self { 
            fuel: 1_000_000_000,        // 1 billion instructions
            memory_bytes: 10 * 1024 * 1024  // 10 MB
        }
    }
}

/// A staging area for state that will be baked into the IsorunCtx.
///
/// System components use this during the linking phase to configure the
/// instance before it's created (WASI preopens, environment variables, etc.).
///
/// Once instantiation is complete, this is consumed to create an `IsorunCtx`.
pub struct ContextBuilder {
    /// WASI configuration (stdio, preopens, env vars, etc.)
    pub wasi: wasmtime_wasi::WasiCtxBuilder,
    /// Type-safe storage for custom user data.
    pub user_data: anymap::Map<dyn anymap::any::Any + Send + Sync>,
}

impl ContextBuilder {
    /// Creates a new empty context builder.
    pub fn new() -> Self {
        Self {
            wasi: wasmtime_wasi::WasiCtxBuilder::new(),
            user_data: anymap::Map::new(),
        }
    }
}

/// The Store context for a running instance.
///
/// This is request-scoped state that lives inside the Wasmtime Store.
/// It does NOT travel across the network - each instance has its own isolated context.
///
/// # Thread Safety
///
/// The entire Store (and thus this context) is protected by a Mutex in
/// `InstanceHandle`, allowing safe concurrent access.
pub struct IsorunCtx {
    pub(crate) wasi: wasmtime_wasi::WasiCtx,
    pub(crate) table: ResourceTable,
    user_data: anymap::Map<dyn anymap::any::Any + Send + Sync>,
}

impl IsorunCtx {
    pub(crate) fn new(mut builder: ContextBuilder) -> Self {
        Self {
            wasi: builder.wasi.build(),
            table: ResourceTable::new(),
            user_data: builder.user_data,
        }
    }

    /// Insert typed data into the context.
    ///
    /// Useful for dependency injection - host functions can retrieve this data.
    pub fn insert<T: Send + Sync + 'static>(&mut self, val: T) {
        self.user_data.insert(val);
    }
    
    /// Get an immutable reference to typed data.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.user_data.get::<T>()
    }
    
    /// Get a mutable reference to typed data.
    pub fn get_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.user_data.get_mut::<T>()
    }
}

impl WasiView for IsorunCtx {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}
