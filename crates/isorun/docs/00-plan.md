<!-- generated -->
<!-- 2025-12-11 -->

# Isorun Implementation Plan

This document outlines the implementation tasks for the isorun runtime, organized in dependency order with verification steps.

## Phase 1: Core System Components

### 1.1 Implement WASI Filesystem Bindings
**File:** `src/system/wasi_fs.rs`

**Task:** Complete the `WasiDir::install()` method to bind WASI filesystem types and preopens.

**Implementation:**
- Add WASI filesystem types to linker using `wasmtime_wasi::bindings::filesystem::types::add_to_linker`
- Add WASI filesystem preopens to linker using `wasmtime_wasi::bindings::filesystem::preopens::add_to_linker`
- Ensure only filesystem capabilities are exposed (no network, clocks, or random)

**Verification:**
```bash
cargo test test_app_wasi_fs
```
Expected: Test passes, app can read/write files in mounted directory.

---

### 1.2 Implement LocalChannel Transport
**File:** `src/system/local_channel.rs`

**Task:** Complete the `LocalChannelTransport` implementation for in-memory, bidirectional communication.

**Implementation:**
- Use `tokio::sync::mpsc` channels to create bidirectional pipes
- Store sender/receiver pairs in the struct
- Implement `Transport::call()` to send payload through one channel and receive response from the other
- Handle channel errors appropriately

**Verification:**
```bash
cargo test test_local_linking
```
Expected: Test passes, two instances can communicate via local channel.

---

## Phase 2: Instance Linking

### 2.1 Implement LocalInstance Linking
**File:** `src/builder.rs` (in `instantiate()` method)

**Task:** Enable direct linking between local Wasm instances in the same process.

**Implementation:**
- In the wiring loop, handle `Linkable::LocalInstance(handle)` case
- Use `linker.func_wrap_async()` to create host functions that:
  - Lock the target instance's store
  - Call the target function directly
  - Return results without serialization
- Extract function signatures from component imports to generate correct wrappers

**Verification:**
```bash
cargo test test_local_linking
```
Expected: Test passes, app_consumer can call app_provider's math functions directly.

---

### 2.2 Implement Remote RPC Stub Generation
**File:** `src/builder.rs` (in `instantiate()` method)

**Task:** Generate RPC stubs for remote instance linking that serialize calls over Transport.

**Implementation:**
- In the wiring loop, handle `Linkable::Remote { transport, remote_instance }` case
- Inspect component imports to extract function signatures
- For each imported function, use `linker.func_wrap_async()` to create a stub that:
  - Serializes arguments using Canonical ABI encoding
  - Prepends `remote_instance` identifier to payload
  - Calls `transport.call(payload).await`
  - Deserializes response using Canonical ABI decoding
  - Returns result to Wasm
- Use `wasmtime::component::types` and `wasmtime::component::Val` for ABI operations

**Verification:**
```bash
cargo test test_remote_linking
```
Expected: Test passes, app_consumer can call remote math service over LocalChannelTransport.

---

## Phase 3: Incoming RPC Handling

### 3.1 Implement Instance Registry
**File:** `src/runtime.rs`

**Task:** Add a registry to track live instances by their remote identifiers.

**Implementation:**
- Add `instances: Mutex<HashMap<String, InstanceHandle>>` to `RuntimeInner`
- Add `Runtime::register_instance(&self, remote_id: String, handle: InstanceHandle)` method
- Add `Runtime::unregister_instance(&self, remote_id: &str)` method
- Update `InstanceBuilder::instantiate()` to optionally register instances with a remote ID

**Verification:**
```bash
cargo test test_instance_registry
```
Write a simple test that registers and retrieves instances by ID.

---

### 3.2 Implement Incoming RPC Handler
**File:** `src/runtime.rs` (in `handle_incoming_rpc()` method)

**Task:** Decode incoming payloads and route to the correct local instance.

**Implementation:**
- Define a simple payload format: `[remote_instance_len: u32][remote_instance: String][function_name_len: u32][function_name: String][canonical_abi_args]`
- Parse the header to extract `remote_instance` and `function_name`
- Look up the target instance from the registry
- Use Canonical ABI to deserialize arguments
- Call the target function on the instance using `InstanceHandle::exec()`
- Serialize the result using Canonical ABI
- Return the response bytes

**Verification:**
```bash
cargo test test_incoming_rpc
```
Expected: Runtime can receive a serialized RPC call and execute it on the correct instance.

---

## Phase 4: Budget and Resource Control

### 4.1 Implement StoreLimits for Budget Enforcement
**Files:** `src/context.rs`, `src/builder.rs`

**Task:** Apply resource limits based on Budget configuration.

**Implementation:**
- Implement `wasmtime::StoreLimits` trait for `IsorunCtx` or a wrapper type
- In `InstanceBuilder::instantiate()`, configure `Store::limiter()` based on `self.budget`
- Map `Budget` fields to Wasmtime limits:
  - `max_memory_bytes` → memory limit
  - `max_table_elements` → table element limit
  - `max_instances` → instance limit
  - `max_tables` → table count limit
  - `max_memories` → memory count limit
- Handle out-of-resources traps gracefully

**Verification:**
```bash
cargo test test_budget_memory_limit
cargo test test_budget_execution_limit
```
Expected: Instances are terminated when they exceed configured limits.

---

## Phase 5: End-to-End Integration

### 5.1 Full Remote RPC Roundtrip
**Task:** Verify complete RPC flow between two Runtime instances.

**Implementation:**
- Create two `Runtime` instances (A and B)
- Use `LocalChannelTransport::pair()` to connect them
- Register a provider instance on B with a remote ID
- Configure B's transport to route incoming calls via `handle_incoming_rpc()`
- Create a consumer instance on A that links to the remote provider on B
- Execute a call and verify the result

**Verification:**
```bash
cargo test test_full_rpc_roundtrip
```
Expected: Complete end-to-end RPC call succeeds across two runtime instances.

---

### 5.2 Multi-System Integration
**Task:** Verify complex instances with multiple system components.

**Implementation:**
- Create an instance that uses:
  - `WasiDir` for filesystem access
  - Local instance linking for direct calls
  - Remote linking for distributed calls
- Verify all capabilities work together correctly

**Verification:**
```bash
cargo test test_multi_system_integration
```
Expected: Instance can use filesystem, local calls, and remote calls simultaneously.

---

## Phase 6: Advanced Features

### 6.1 Bi-directional RPC
**Task:** Enable instances to both provide and consume remote functions.

**Implementation:**
- Allow instances to be both servers (registered with remote IDs) and clients (with remote links)
- Verify deadlock-free execution when instances call each other

**Verification:**
```bash
cargo test test_bidirectional_rpc
```
Expected: Two instances can call each other's exported functions.

---

### 6.2 Custom System Components
**Task:** Document and test the process for users to implement custom `SystemComponent` traits.

**Implementation:**
- Create example custom system components (e.g., a key-value store, authentication service)
- Document the `install()` and `configure()` lifecycle
- Test integration with instance builder

**Verification:**
```bash
cargo test test_custom_system_component
```
Expected: Custom system component can be linked and used by instances.

---

## Testing Strategy

After each implementation step:
1. Run `cargo check` to verify compilation
2. Run the specific verification test(s) listed
3. Run `cargo test` to ensure no regressions
4. Update this document with any new findings or required changes

## Success Criteria

The implementation is complete when:
- All tests in `tests/integration_tests.rs` pass (currently 5 are ignored)
- `cargo test` shows 0 failures
- All TODO comments in the codebase are resolved
- Documentation is complete for public APIs
