//! Low-level tests for component export navigation.
//!
//! These tests demonstrate the proper way to call functions exported from
//! component model interfaces using export indices.

use std::sync::Arc;
use std::sync::Mutex;

use wasmtime::component::Linker;
use wasmtime::component::types::ComponentItem;

use exorun::context::ExorunCtx;
use exorun::runtime::Runtime;
use exorun::system::{SystemTarget, WasiSystem};

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

// Simple logger system for testing
#[derive(Clone)]
struct TestLogger {
    logs: Arc<Mutex<Vec<String>>>,
}

impl TestLogger {
    fn new() -> Self {
        Self {
            logs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn get_logs(&self) -> Vec<String> {
        self.logs.lock().unwrap().clone()
    }

    fn install(&self, linker: &mut Linker<ExorunCtx>) -> Result<(), exorun::system::Error> {
        let logs = self.logs.clone();

        let mut instance = linker
            .instance("test:demo/logging")
            .map_err(|e| exorun::system::Error::Linker(e.to_string()))?;

        instance
            .func_wrap(
                "log",
                move |_caller: wasmtime::StoreContextMut<'_, ExorunCtx>,
                      (level, msg): (String, String)| {
                    logs.lock().unwrap().push(format!("[{}] {}", level, msg));
                    Ok(())
                },
            )
            .map_err(|e| exorun::system::Error::Linker(e.to_string()))?;

        Ok(())
    }
}

/// Test that demonstrates the low-level wasmtime API for calling exported interface functions.
///
/// This test shows:
/// 1. How component model exports work when a world exports an interface
/// 2. The proper navigation pattern using export indices
/// 3. The difference between ComponentInstance exports and direct function exports
#[tokio::test]
async fn test_low_level_component_export_navigation() {
    // Create runtime and load app_logger component
    let rt = Runtime::new().expect("Failed to create runtime");
    let wasm_bytes = std::fs::read(fixture_path("app_logger.wasm"))
        .expect("Failed to read app_logger.wasm");

    let component = wasmtime::component::Component::new(rt.engine(), &wasm_bytes)
        .expect("Failed to compile component");

    let logger = TestLogger::new();

    // Set up linker with system components
    let rt = std::sync::Arc::new(rt);
    let mut linker = Linker::new(rt.engine());
    let mut ctx_builder = exorun::context::ContextBuilder::new();
    
    logger.install(&mut linker).expect("Failed to install logger");
    SystemTarget::Wasi(WasiSystem::new()).link(&mut linker, &mut ctx_builder).expect("Failed to link WASI");

    // Create store with runtime context
    let ctx = ctx_builder.build(std::sync::Arc::clone(&rt));
    let mut store = wasmtime::Store::new(rt.engine(), ctx);

    // Instantiate the component
    let instance = linker
        .instantiate_async(&mut store, &component)
        .await
        .expect("Failed to instantiate");

    // Get the exported "test:demo/runnable" interface - it's exported as an instance
    let runnable_export = instance
        .get_export(&mut store, None, "test:demo/runnable")
        .expect("Failed to get runnable export");

    // The export is a ComponentInstance (nested export)
    let (item, idx) = runnable_export;
    let ComponentItem::ComponentInstance(_inst_ty) = item else {
        panic!("Expected test:demo/runnable to be a ComponentInstance");
    };

    // To get a function from within an exported instance, we need to:
    // 1. Get the export index for the instance (we already have this: idx)
    // 2. Get the export index for the function within that instance
    // 3. Use the function index to get the actual function

    // Now get the function export index from within that instance
    let func_idx = component
        .get_export_index(Some(&idx), "run")
        .expect("Failed to get run function index");

    // Now get the actual function using the index
    let run_func = instance
        .get_func(&mut store, &func_idx)
        .expect("Failed to get run function");

    // Type it and call it
    let typed = run_func
        .typed::<(), (String,)>(&mut store)
        .expect("Type check failed");
    let (result,) = typed.call_async(&mut store, ()).await.expect("Call failed");

    // Verify the function returned successfully
    assert_eq!(result, "Done", "Function should return 'Done'");

    // Verify the log was captured
    let logs = logger.get_logs();
    assert_eq!(logs.len(), 1, "Expected exactly one log entry");
    assert_eq!(logs[0], "[INFO] Hello from Wasm!", "Log message mismatch");
}
