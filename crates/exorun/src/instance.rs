//! # Instance Handle
//!
//! Provides a thread-safe handle to a running Wasm instance. Encapsulates the
//! Store and Instance in a mutex to allow async operations from multiple tasks.

use std::sync::Arc;

use tokio::sync::Mutex;
use wasmtime::Store;
use wasmtime::component::Val;
use wasmtime::component::Instance;

use crate::context::ExorunCtx;

#[derive(Debug)]
pub enum Error {
    Execution(wasmtime::Error),
    Lock(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Execution(e) => write!(f, "Execution error: {}", e),
            Self::Lock(msg) => write!(f, "Lock error: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

impl From<wasmtime::Error> for Error {
    fn from(e: wasmtime::Error) -> Self {
        Self::Execution(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Thread-safe handle to a Wasm instance.
///
/// Wasmtime's Store is !Send + !Sync, so we wrap it in Arc<Mutex<...>> to enable
/// async access from multiple tasks. This allows one instance to call into another
/// instance without data races.
#[derive(Clone)]
pub struct LocalTarget {
    pub(crate) inner: Arc<Mutex<State>>,
}

pub(crate) struct State {
    pub store: Store<ExorunCtx>,
    pub instance: Instance,
}

impl LocalTarget {
    /// Creates a new instance handle wrapping the store and instance.
    pub fn new(store: Store<ExorunCtx>, instance: Instance) -> Self {
        Self {
            inner: Arc::new(Mutex::new(State { store, instance })),
        }
    }

    /// Calls an exported function from an interface dynamically using Vals.
    ///
    /// When a component exports an interface (e.g., `export test:demo/runnable`),
    /// the functions within that interface must be accessed via the component's
    /// export indices. This method uses dynamic Val types to support any function
    /// signature without compile-time type constraints.
    ///
    /// # Arguments
    /// * `component` - The component definition containing export metadata
    /// * `interface` - The interface name (e.g., "test:demo/runnable")
    /// * `function` - The function name within the interface (e.g., "run")
    /// * `args` - The arguments to pass to the function as Vals
    /// * `results` - A mutable slice to receive the function results as Vals
    ///
    /// # Example
    /// ```ignore
    /// let mut results = vec![Val::String(String::new())];
    /// handle.call_interface_func(
    ///     &component,
    ///     "test:demo/runnable",
    ///     "run",
    ///     &[],
    ///     &mut results
    /// ).await?;
    /// ```
    pub async fn call_interface_func(
        &self,
        component: &wasmtime::component::Component,
        interface: &str,
        function: &str,
        args: &[Val],
        results: &mut [Val],
    ) -> Result<()> {
        // Get the export indices first (before the async block)
        let inst_idx = component
            .get_export_index(None, interface)
            .ok_or_else(|| Error::Execution(
                wasmtime::Error::msg(format!("Interface '{}' not found", interface))
            ))?;

        let func_idx = component
            .get_export_index(Some(&inst_idx), function)
            .ok_or_else(|| Error::Execution(
                wasmtime::Error::msg(format!("Function '{}' not found in interface '{}'", function, interface))
            ))?;

        // Now use the indices to get and call the function
        let mut guard = self.inner.lock().await;
        let State { store, instance } = &mut *guard;

        let func = instance
            .get_func(&mut *store, &func_idx)
            .ok_or_else(|| Error::Execution(
                wasmtime::Error::msg("Failed to get function".to_string())
            ))?;

        // Call the function with dynamic Vals
        func.call_async(&mut *store, args, results)
            .await
            .map_err(Error::from)
    }

    /// Executes a closure with exclusive access to the store and instance.
    ///
    /// This handles the locking ceremony, allowing the caller to operate on the
    /// StoreContextMut and Instance safely. The lock is released when the closure
    /// completes.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = handle.exec(|store, instance| {
    ///     let func = instance.get_typed_func::<(), ()>(store, "run")?;
    ///     func.call(store, ())
    /// }).await?;
    /// ```
    pub async fn exec<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut Store<ExorunCtx>, &Instance) -> Result<R>,
    {
        let mut guard = self.inner.lock().await;
        let State { store, instance } = &mut *guard;
        f(store, instance)
    }

    /// Executes an async closure with exclusive access to the store and instance.
    ///
    /// This is the async version of `exec`, allowing the closure to perform
    /// async operations (like calling async Wasm functions) while holding the lock.
    pub async fn exec_async<F, Fut, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut Store<ExorunCtx>, &Instance) -> Fut,
        Fut: std::future::Future<Output = Result<R>>,
    {
        let mut guard = self.inner.lock().await;
        let State { store, instance } = &mut *guard;
        f(store, instance).await
    }
}
