//! Context types for instance configuration and execution

use wasmtime::component::ResourceTable;

/// Configuration and budget for instances
#[derive(Clone, Debug)]
pub struct Budget {
    pub fuel: u64,
    pub memory_bytes: u64,
}

impl Budget {
    pub fn standard() -> Self {
        Self { 
            fuel: 1_000_000_000, 
            memory_bytes: 10 * 1024 * 1024 
        }
    }
}

/// A staging area for state that will be baked into the IsorunCtx.
/// This allows systems to inject configuration (WASI preopens, Auth tokens)
/// before the Store is actually created.
pub struct ContextBuilder {
    pub wasi: wasmtime_wasi::WasiCtxBuilder,
    pub user_data: anymap::Map<dyn anymap::any::Any + Send + Sync>,
}

impl ContextBuilder {
    pub fn new() -> Self {
        Self {
            wasi: wasmtime_wasi::WasiCtxBuilder::new(),
            user_data: anymap::Map::new(),
        }
    }
}

/// Request-scoped state. This does NOT travel across the network.
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

    pub fn insert<T: Send + Sync + 'static>(&mut self, val: T) {
        self.user_data.insert(val);
    }
    
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.user_data.get::<T>()
    }
    
    pub fn get_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.user_data.get_mut::<T>()
    }
}
