//! Handle to a live, thread-safe running instance.
//!
//! An `InstanceHandle` provides safe concurrent access to a running WebAssembly
//! instance. The instance's Store is protected by a Mutex, allowing multiple
//! async tasks to interact with it safely.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
use wasmtime::AsContextMut;
use wasmtime::Store;
use wasmtime::StoreContextMut;

use crate::context::IsorunCtx;

/// A handle to a live, thread-safe running instance.
///
/// This is a cheap-to-clone handle that allows safe concurrent access to a
/// WebAssembly instance. Multiple tasks can hold handles to the same instance.
///
/// # Concurrency
///
/// The Store is protected by an `Arc<Mutex<...>>`, so only one task can execute
/// on the instance at a time. Other tasks will wait for the lock to be released.
#[derive(Clone)]
pub struct InstanceHandle {
    pub(crate) store: Arc<Mutex<Store<IsorunCtx>>>,
    pub(crate) instance: wasmtime::component::Instance,
}

impl InstanceHandle {
    /// Execute a function on this instance.
    ///
    /// This locks the instance's store for the duration of the closure.
    /// Other tasks attempting to access this instance will block.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use isorun::InstanceHandle;
    /// # async fn example(handle: InstanceHandle) -> anyhow::Result<()> {
    /// handle.exec(|mut store, instance| async move {
    ///     let func = instance.get_typed_func::<(), ()>(&mut store, "run")?;
    ///     func.call_async(&mut store, ()).await?;
    ///     Ok(())
    /// }).await?;
    /// # Ok(())
    /// # }
    /// ```
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

    /// Inject typed data into the instance's context.
    ///
    /// This is useful for dependency injection - host functions can later
    /// retrieve this data via `ctx.get::<T>()`.
    pub async fn set_context<T: Send + Sync + 'static>(&self, val: T) {
        let mut lock = self.store.lock().await;
        lock.data_mut().insert(val);
    }
}
