//! # Logger host component
//!
//! Provides a simple logging interface for Wasm components.
//! Captures log messages in memory for testing and inspection.

use std::sync::Arc;

use tokio::sync::Mutex;
use wasmtime::component::Linker;

use crate::context::ExorunCtx;
use crate::host::Result;

/// Logger host component that captures log messages.
///
/// Provides the `exorun:host/logging` interface to Wasm components,
/// allowing them to emit log messages that are captured in memory.
#[derive(Clone, Debug)]
pub struct Logger {
    logs: Arc<Mutex<Vec<String>>>,
}

impl Logger {
    /// Creates a new logger instance.
    pub fn new() -> Self {
        Self {
            logs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Returns all captured log messages.
    pub async fn get_logs(&self) -> Vec<String> {
        self.logs.lock().await.clone()
    }

    /// Links this logger to the linker, installing the `exorun:host/logging` interface.
    pub fn link(&self, linker: &mut Linker<ExorunCtx>) -> Result<()> {
        let logs = self.logs.clone();

        let mut instance = linker
            .instance("exorun:host/logging")
            .map_err(|e| crate::host::Error::Link(e.to_string()))?;

        instance
            .func_wrap(
                "log",
                move |_caller: wasmtime::StoreContextMut<'_, ExorunCtx>,
                      (level, msg): (String, String)| {
                    let mut guard = logs.try_lock()
                        .map_err(|_| wasmtime::Error::msg("logger mutex contention"))?;
                    guard.push(format!("[{}] {}", level, msg));
                    Ok(())
                },
            )
            .map_err(|e| crate::host::Error::Link(e.to_string()))?;

        Ok(())
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::new()
    }
}
