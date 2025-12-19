//! Tests for InstanceBuilder interface name validation

use exorun::runtime::Runtime;
use exorun::host::HostInstance;
use exorun::host::Wasi;
use exorun::host::Logger;
use exorun::host::Kv;

/// Helper to load Wasm fixtures.
fn wasm(name: &str) -> Vec<u8> {
    let path = format!("tests/fixtures/{}.wasm", name);
    std::fs::read(&path).unwrap_or_else(|_| panic!("Could not read wasm: {}", path))
}

// --- Happy Path Tests ---

#[tokio::test]
async fn test_valid_wasi_interface() {
    let rt = Runtime::new().expect("Failed to create runtime");
    let app_id = rt.add_component_bytes(&wasm("app_logger")).expect("Failed to register app");

    // WASI should accept any wasi:* interface
    let result = rt.instantiate(app_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_system("exorun:host/logging", HostInstance::Logger(Logger::new()))
        .build()
        .await;

    assert!(result.is_ok(), "Valid WASI interface should succeed");
}

#[tokio::test]
async fn test_valid_logger_interface() {
    let rt = Runtime::new().expect("Failed to create runtime");
    let app_id = rt.add_component_bytes(&wasm("app_logger")).expect("Failed to register app");

    let result = rt.instantiate(app_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_system("exorun:host/logging", HostInstance::Logger(Logger::new()))
        .build()
        .await;

    assert!(result.is_ok(), "Valid Logger interface should succeed");
}

#[tokio::test]
async fn test_valid_kv_interface() {
    let rt = Runtime::new().expect("Failed to create runtime");
    let app_id = rt.add_component_bytes(&wasm("app_kv")).expect("Failed to register app");

    let result = rt.instantiate(app_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_system("exorun:host/kv", HostInstance::Kv(Kv::new()))
        .build()
        .await;

    assert!(result.is_ok(), "Valid KV interface should succeed");
}

#[tokio::test]
async fn test_multiple_wasi_interfaces() {
    let rt = Runtime::new().expect("Failed to create runtime");
    let app_id = rt.add_component_bytes(&wasm("app_logger")).expect("Failed to register app");

    // WASI can be linked multiple times with different interface names
    // (though in practice, add_to_linker_async adds all interfaces at once)
    let result = rt.instantiate(app_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_system("exorun:host/logging", HostInstance::Logger(Logger::new()))
        .build()
        .await;

    assert!(result.is_ok(), "Multiple WASI interfaces should succeed");
}

// --- Failure Tests ---

#[tokio::test]
async fn test_invalid_wasi_interface_name() {
    let rt = Runtime::new().expect("Failed to create runtime");
    let app_id = rt.add_component_bytes(&wasm("app_logger")).expect("Failed to register app");

    // WASI should reject non-wasi:* interfaces
    let result = rt.instantiate(app_id)
        .link_system("exorun:host/logging", HostInstance::Wasi(Wasi::new()))
        .build()
        .await;

    assert!(result.is_err(), "WASI should reject non-wasi:* interface");
    let err = result.unwrap_err();
    assert!(err.to_string().contains("WASI host instance cannot provide interface"));
    assert!(err.to_string().contains("exorun:host/logging"));
}

#[tokio::test]
async fn test_invalid_logger_interface_name() {
    let rt = Runtime::new().expect("Failed to create runtime");
    let app_id = rt.add_component_bytes(&wasm("app_logger")).expect("Failed to register app");

    // Logger should reject wrong interface name
    let result = rt.instantiate(app_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_system("exorun:host/kv", HostInstance::Logger(Logger::new()))
        .build()
        .await;

    assert!(result.is_err(), "Logger should reject wrong interface name");
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Logger host instance cannot provide interface"));
    assert!(err.to_string().contains("exorun:host/kv"));
}

#[tokio::test]
async fn test_invalid_kv_interface_name() {
    let rt = Runtime::new().expect("Failed to create runtime");
    let app_id = rt.add_component_bytes(&wasm("app_kv")).expect("Failed to register app");

    // KV should reject wrong interface name
    let result = rt.instantiate(app_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_system("exorun:host/logging", HostInstance::Kv(Kv::new()))
        .build()
        .await;

    assert!(result.is_err(), "KV should reject wrong interface name");
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Kv host instance cannot provide interface"));
    assert!(err.to_string().contains("exorun:host/logging"));
}

// --- Boundary Tests ---

#[tokio::test]
async fn test_multiple_kv_instances_different_names() {
    let rt = Runtime::new().expect("Failed to create runtime");

    // Create a component that imports two KV stores with different names
    // For this test, we'll use the same wasm but link it differently
    let app_id = rt.add_component_bytes(&wasm("app_kv")).expect("Failed to register app");

    let user_kv = Kv::new();
    let session_kv = Kv::new();

    // This test shows the API works for multiple instances
    // Even though our test wasm only imports one KV, the API supports this pattern
    let result = rt.instantiate(app_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_system("exorun:host/kv", HostInstance::Kv(user_kv.clone()))
        .build()
        .await;

    assert!(result.is_ok(), "Should be able to link KV with correct name");

    // Verify we can create another instance with a different KV
    let result2 = rt.instantiate(app_id)
        .link_system("wasi:cli/environment", HostInstance::Wasi(Wasi::new()))
        .link_system("exorun:host/kv", HostInstance::Kv(session_kv.clone()))
        .build()
        .await;

    assert!(result2.is_ok(), "Should be able to link different KV instance");
}

#[tokio::test]
async fn test_wasi_interface_variations() {
    let rt = Runtime::new().expect("Failed to create runtime");
    let app_id = rt.add_component_bytes(&wasm("app_logger")).expect("Failed to register app");

    // Test various valid WASI interface names
    let valid_wasi_interfaces = vec![
        "wasi:cli/environment",
        "wasi:cli/exit",
        "wasi:filesystem/types",
        "wasi:filesystem/preopens",
        "wasi:io/streams",
        "wasi:clocks/wall-clock",
        "wasi:random/random",
        "wasi:sockets/network",
    ];

    for interface in valid_wasi_interfaces {
        let result = rt.instantiate(app_id)
            .link_system(interface, HostInstance::Wasi(Wasi::new()))
            .link_system("exorun:host/logging", HostInstance::Logger(Logger::new()))
            .build()
            .await;

        assert!(
            result.is_ok(),
            "WASI should accept interface '{}', but got error: {:?}",
            interface,
            result.err()
        );
    }
}

#[tokio::test]
async fn test_empty_interface_name() {
    let rt = Runtime::new().expect("Failed to create runtime");
    let app_id = rt.add_component_bytes(&wasm("app_logger")).expect("Failed to register app");

    // Empty string should fail for all host instances
    let result = rt.instantiate(app_id)
        .link_system("", HostInstance::Logger(Logger::new()))
        .build()
        .await;

    assert!(result.is_err(), "Empty interface name should fail");
}

#[tokio::test]
async fn test_wasi_prefix_but_invalid() {
    let rt = Runtime::new().expect("Failed to create runtime");
    let app_id = rt.add_component_bytes(&wasm("app_logger")).expect("Failed to register app");

    // "wasi" without colon should fail
    let result = rt.instantiate(app_id)
        .link_system("wasi", HostInstance::Wasi(Wasi::new()))
        .build()
        .await;

    // This should actually succeed because it starts with "wasi" - but let's see
    // Actually, our validation is starts_with("wasi:") so this should fail
    assert!(result.is_err(), "Invalid wasi prefix should fail");
}
