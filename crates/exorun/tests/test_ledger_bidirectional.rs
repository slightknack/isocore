//! Tests for bidirectional ledger validation and export index pre-computation

use exorun::Runtime;
use exorun::host::{HostInstance, Wasi};

/// Helper to load Wasm fixtures.
fn wasm(name: &str) -> Vec<u8> {
    let path = format!("tests/fixtures/{}.wasm", name);
    std::fs::read(&path).unwrap_or_else(|_| panic!("Could not read wasm: {}", path))
}

/// Test that ledger is stored per-component in runtime
#[tokio::test]
async fn test_runtime_stores_ledger_per_component() {
    let runtime = Runtime::new().expect("runtime creation failed");
    
    let bytes1 = wasm("app_provider");
    let bytes2 = wasm("app_consumer");
    
    let id1 = runtime.add_component_bytes(&bytes1).expect("add component 1");
    let id2 = runtime.add_component_bytes(&bytes2).expect("add component 2");
    
    // Should be able to get ledgers for both components
    let _ledger1 = runtime.get_ledger(id1).expect("get ledger 1");
    let _ledger2 = runtime.get_ledger(id2).expect("get ledger 2");
    
    // Test passes if we successfully retrieved both ledgers without error
}

/// Test bidirectional validation: successful local link
#[tokio::test]
async fn test_bidirectional_validation_success() {
    let rt = Runtime::new().expect("runtime creation failed");

    let provider_id = rt.add_component_bytes(&wasm("app_provider")).expect("add provider");
    let consumer_id = rt.add_component_bytes(&wasm("app_consumer")).expect("add consumer");

    let provider_inst = rt.instantiate(provider_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .build()
        .await
        .expect("instantiate provider");

    // This should succeed - bidirectional validation passes
    let _consumer_inst = rt.instantiate(consumer_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_local("exorun:test/math", provider_inst)
        .build()
        .await
        .expect("bidirectional validation should pass");
}

/// Test bidirectional validation: target doesn't export interface
#[tokio::test]
async fn test_bidirectional_validation_missing_export() {
    let runtime = Runtime::new().expect("runtime creation failed");
    
    let provider_bytes = wasm("app_provider");
    let consumer_bytes = wasm("app_consumer");
    
    let provider_id = runtime.add_component_bytes(&provider_bytes).expect("add provider");
    let consumer_id = runtime.add_component_bytes(&consumer_bytes).expect("add consumer");
    
    // Instantiate provider
    let provider_instance = runtime.instantiate(provider_id)
        .build()
        .await
        .expect("instantiate provider");
    
    // Try to link consumer to a non-existent interface
    // This should fail during validation
    let result = runtime.instantiate(consumer_id)
        .link_local("nonexistent:interface/api", provider_instance)
        .build()
        .await;
    
    // Should fail - either during our validation or wasmtime's
    assert!(result.is_err(), "should fail when linking to non-existent interface");
}

/// Test export index pre-computation enables fast calls
#[tokio::test]
async fn test_export_index_precomputation() {
    let rt = Runtime::new().expect("runtime creation failed");
    
    let provider_id = rt.add_component_bytes(&wasm("app_provider")).expect("add provider");
    
    let provider_inst = rt.instantiate(provider_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .build()
        .await
        .expect("instantiate provider");
    
    // Call the exported add function - this uses pre-computed export indices
    let result = rt.call(
        provider_inst,
        "exorun:test/math",
        "add",
        &[wasmtime::component::Val::U32(5), wasmtime::component::Val::U32(3)]
    ).await.expect("call should succeed with pre-computed indices");
    
    // Verify we got the correct result
    assert_eq!(result.len(), 1);
    match &result[0] {
        wasmtime::component::Val::U32(v) => assert_eq!(*v, 8),
        _ => panic!("expected u32 result"),
    }
}

/// Test that calling non-existent function fails correctly
#[tokio::test]
async fn test_call_nonexistent_function() {
    let runtime = Runtime::new().expect("runtime creation failed");
    
    let provider_bytes = wasm("app_provider");
    let provider_id = runtime.add_component_bytes(&provider_bytes).expect("add provider");
    
    let provider_instance = runtime.instantiate(provider_id)
        .build()
        .await
        .expect("instantiate provider");
    
    // Try to call non-existent function
    let result = runtime.call(provider_instance, "exorun:test/provider", "doesnt-exist", &[]).await;
    
    assert!(result.is_err(), "should fail calling non-existent function");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("not found") || err_msg.contains("doesnt-exist"),
        "error should mention missing function: {}",
        err_msg
    );
}

/// Test that calling with wrong interface fails correctly  
#[tokio::test]
async fn test_call_wrong_interface() {
    let runtime = Runtime::new().expect("runtime creation failed");
    
    let provider_bytes = wasm("app_provider");
    let provider_id = runtime.add_component_bytes(&provider_bytes).expect("add provider");
    
    let provider_instance = runtime.instantiate(provider_id)
        .build()
        .await
        .expect("instantiate provider");
    
    // Try to call with wrong interface name
    let result = runtime.call(provider_instance, "wrong:interface/name", "get", &[]).await;
    
    assert!(result.is_err(), "should fail calling wrong interface");
}

/// Test that ledger correctly identifies imports vs exports
#[tokio::test]
async fn test_ledger_imports_vs_exports() {
    let runtime = Runtime::new().expect("runtime creation failed");
    
    let provider_bytes = wasm("app_provider");
    let consumer_bytes = wasm("app_consumer");
    
    let provider_id = runtime.add_component_bytes(&provider_bytes).expect("add provider");
    let consumer_id = runtime.add_component_bytes(&consumer_bytes).expect("add consumer");
    
    let provider_ledger = runtime.get_ledger(provider_id).expect("get provider ledger");
    let consumer_ledger = runtime.get_ledger(consumer_id).expect("get consumer ledger");
    
    // Provider should have some exports (the fixtures are designed to export interfaces)
    assert!(
        !provider_ledger.exports.is_empty(),
        "provider should have at least one export"
    );
    
    // Consumer should have some imports (the fixtures are designed to import interfaces)
    assert!(
        !consumer_ledger.imports.is_empty(),
        "consumer should have at least one import"
    );
    
    // Check if there's overlap (consumer imports what provider exports)
    let has_matching_interface = consumer_ledger.imports.keys()
        .any(|import_name| provider_ledger.exports.contains_key(import_name));
    
    assert!(
        has_matching_interface,
        "consumer should import an interface that provider exports"
    );
}
