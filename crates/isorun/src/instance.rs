//! Handle to a live, thread-safe running instance

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
use wasmtime::AsContextMut;
use wasmtime::Store;
use wasmtime::StoreContextMut;

use crate::context::IsorunCtx;

/// A handle to a live, thread-safe running instance.
#[derive(Clone)]
pub struct InstanceHandle {
    pub(crate) store: Arc<Mutex<Store<IsorunCtx>>>,
    pub(crate) instance: wasmtime::component::Instance,
}

impl InstanceHandle {
    /// Execute a function on this instance.
    ///
    /// This locks the instance's store (and only this instance).
    /// You provide a closure that uses the `bindgen!` generated code.
    pub async fn exec<F, Fut, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut StoreContextMut<IsorunCtx>, &wasmtime::component::Instance) -> Fut + Send,
        Fut: std::future::Future<Output = Result<R>> + Send,
        R: Send,
    {
        let mut lock = self.store.lock().await;
        let ctx = &mut lock.as_context_mut();
        f(ctx, &self.instance).await
    }

    /// Helper to inject context data before running.
    pub async fn set_context<T: Send + Sync + 'static>(&self, val: T) {
        let mut lock = self.store.lock().await;
        lock.data_mut().insert(val);
    }
}
