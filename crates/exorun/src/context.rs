//! # Wasmtime instance context for local components
//!
//! Store context for running component instances.

use std::sync::Arc;

use wasmtime::component::ResourceTable;
use wasmtime_wasi::WasiCtx;
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::WasiCtxView;
use wasmtime_wasi::WasiView;

use crate::runtime::Runtime;

/// Builder for constructing an ExorunCtx.
///
/// Provides a fluent API for configuring WASI capabilities and user data
/// before finalizing the context for instance execution.
pub struct ContextBuilder {
    pub wasi: WasiCtxBuilder,
    pub user_data: anymap::Map<dyn anymap::any::Any + Send + Sync>,
}

impl ContextBuilder {
    pub fn new() -> Self {
        Self {
            wasi: WasiCtxBuilder::new(),
            user_data: anymap::Map::new(),
        }
    }

    pub fn inherit_stdio(mut self) -> Self {
        self.wasi.inherit_stdio();
        self
    }

    pub fn inherit_env(mut self) -> Self {
        self.wasi.inherit_env();
        self
    }

    pub fn env(mut self, key: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        self.wasi.env(key.as_ref(), value.as_ref());
        self
    }

    pub fn insert<T: anymap::any::Any + Send + Sync>(&mut self, data: T) {
        self.user_data.insert(data);
    }

    pub fn build(mut self, runtime: Arc<Runtime>) -> ExorunCtx {
        ExorunCtx {
            wasi: self.wasi.build(),
            table: ResourceTable::new(),
            user_data: self.user_data,
            runtime,
        }
    }
}

impl Default for ContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-instance execution context stored in Wasmtime's Store.
///
/// Holds mutable state scoped to a single component instance. Provides:
/// - WASI capabilities (filesystem, environment, stdio)
/// - Resource table for WASI resource management
/// - Type-safe user data injection via AnyMap
/// - Reference to the global Runtime for peer resolution and meta operations
pub struct ExorunCtx {
    pub(crate) wasi: WasiCtx,
    pub(crate) table: ResourceTable,
    pub(crate) user_data: anymap::Map<dyn anymap::any::Any + Send + Sync>,
    pub(crate) runtime: Arc<Runtime>,
}

impl ExorunCtx {
    /// Retrieves user data by type.
    pub fn get<T: anymap::any::Any + Send + Sync>(&self) -> Option<&T> {
        self.user_data.get::<T>()
    }
}

impl WasiView for ExorunCtx {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}
