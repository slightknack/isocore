//! # Key-Value store host component
//!
//! Provides a simple in-memory key-value store for Wasm components.
//! Useful for testing and stateful system integration.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use wasmtime::component::Linker;

use crate::context::ExorunCtx;
use crate::host::Result;

/// Key-Value store host component.
///
/// Provides the `exorun:host/kv` interface to Wasm components,
/// implementing a simple in-memory string-to-string mapping.
#[derive(Clone, Debug)]
pub struct Kv {
    store: Arc<Mutex<HashMap<String, String>>>,
}

impl Kv {
    /// Creates a new empty key-value store.
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Returns a snapshot of all key-value pairs in the store.
    pub fn get_store(&self) -> HashMap<String, String> {
        self.store.lock().unwrap().clone()
    }

    /// Links this KV store to the linker, installing the `exorun:host/kv` interface.
    pub fn link(&self, linker: &mut Linker<ExorunCtx>) -> Result<()> {
        let store = self.store.clone();

        let mut instance = linker
            .instance("exorun:host/kv")
            .map_err(|e| crate::host::Error::Link(e.to_string()))?;

        // Bind the 'get' function
        instance
            .func_wrap(
                "get",
                {
                    let store = store.clone();
                    move |_caller: wasmtime::StoreContextMut<'_, ExorunCtx>, (key,): (String,)| {
                        let value = store.lock().unwrap().get(&key).cloned();
                        Ok((value,))
                    }
                },
            )
            .map_err(|e| crate::host::Error::Link(e.to_string()))?;

        // Bind the 'set' function
        instance
            .func_wrap(
                "set",
                move |_caller: wasmtime::StoreContextMut<'_, ExorunCtx>,
                      (key, val): (String, String)| {
                    store.lock().unwrap().insert(key, val);
                    Ok(())
                },
            )
            .map_err(|e| crate::host::Error::Link(e.to_string()))?;

        Ok(())
    }
}

impl Default for Kv {
    fn default() -> Self {
        Self::new()
    }
}
