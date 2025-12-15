---
origin: generated
date: 2025-12-14
---

# Implementation Plan for Exorun

This document outlines the architectural path to complete the implementation of `exorun`. We will establish a rigorous engineering standard inspired by "Tigerstyle" and the pragmatism of Go, treating the codebase as a high-reliability construction project. We favor flat control flow, explicit error handling, and high-concurrency data structures to ensure the runtime is robust under pressure.

## Engineering Philosophy and Style Guide

We adhere to a specific dialect of Rust that prioritizes readability and linear cognitive load. Write code that looks like C with safety guarantees, or as a Go programmer who has discovered generics and `Result` types. Avoid deep nesting ("arrow code"); instead of wrapping logic in layers of `if let` or `match`, invert the control flow. Use `let else` guard clauses to handle error cases and `None` variants early, returning or breaking immediately. This keeps the "happy path" aligned to the left margin, making the logic flow down the screen like a waterfall rather than expanding outward like a pyramid.

Functions should be focused and shallow. If a loop body contains complex logic, extract it into a dedicated function. When defining errors, every module must define its own `Error` enum. When importing errors from other modules, never alias them (e.g., do not use `use crate::transport::Error as TransportError`). Instead, import the module and namespace the error usage to preserve context, writing `transport::Error` in function signatures. This makes the origin of failure explicitly clear at the call site.

## Dependency Management

To support the concurrent requirements of the runtime registry and the storage needs of the context, we must add high-performance containers. We replace standard mutex-protected hashmaps with concurrent maps to reduce contention in high-throughput scenarios.

Add `dashmap` and `anymap` to `crates/exorun/Cargo.toml`. `dashmap` provides shard-locked concurrent hashmaps, allowing the runtime to register apps and peers without global locking. `anymap` allows the context to act as a type-safe dependency injection container for user-provided data.

```toml
[dependencies]
# ... existing dependencies
dashmap = "6.0"
anymap = "1.0"
```

## Error Handling Refactor

We must standardize error handling across the codebase to match our style guide. Open `crates/exorun/src/client.rs`. The current implementation likely aliases errors or imports them directly. Refactor the `Error` enum variants to wrap external errors cleanly, but in the implementation of methods, refer to external errors by their module path.

Perform this refactor across `transport.rs`, `ledger.rs`, and `bind.rs`. If a file has potential failure modes, it defines `pub enum Error`. If `bind.rs` needs to handle a transport failure, it should match on `transport::Error`, not an aliased name. This strictly couples the error type to its domain boundary.

```rust
// crates/exorun/src/client.rs

use crate::transport;
use crate::neorpc;

#[derive(Debug)]
pub enum Error {
    Transport(transport::Error),
    Rpc(neorpc::Error),
    // ...
}

// In usage:
// fn call(...) -> Result<...> {
//     self.transport.call(payload).await.map_err(transport::Error::into)
// }
```

## Context Enhancement

The `Context` is the bedrock of state for a WebAssembly instance. We must upgrade `crates/exorun/src/context.rs` to support the WebAssembly System Interface (WASI) and arbitrary user data. This transforms `ExorunCtx` from a simple sequence counter into a fully-fledged operating environment.

We define a `ContextBuilder` to separate configuration (staging) from execution (state). The builder accumulates environment variables, standard I/O redirection, and preopened directories. The final `ExorunCtx` holds the `WasiCtx`, the `ResourceTable` (required for resource management), and an `AnyMap` for user data injection. Implement `WasiView` for `ExorunCtx` to satisfy `wasmtime-wasi` requirements.

```rust
// crates/exorun/src/context.rs

use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiView, WasiCtxBuilder};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct ContextBuilder {
    pub wasi: WasiCtxBuilder,
    pub user_data: anymap::Map<dyn anymap::any::Any + Send + Sync>,
}

pub struct ExorunCtx {
    seq: AtomicU64,
    pub(crate) wasi: WasiCtx,
    pub(crate) table: ResourceTable,
    pub(crate) user_data: anymap::Map<dyn anymap::any::Any + Send + Sync>,
}

impl WasiView for ExorunCtx {
    fn ctx(&mut self) -> wasmtime_wasi::WasiCtxView<'_> {
        wasmtime_wasi::WasiCtxView::new(&mut self.wasi, &mut self.table)
    }
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}
```

## System Component Interface

Create `crates/exorun/src/system.rs`. This module defines the contract for native Rust code that exposes functionality to Wasm components. A system component must be able to install itself into a linker (defining the interface) and configure a context builder (provisioning resources like file descriptors).

Define the `SystemComponent` trait. Implement a concrete `WasiSystem` component within this module. This component is responsible for mapping host directories to guest directories. It keeps the "what" (interface) separate from the "how" (implementation details like path mapping).

```rust
// crates/exorun/src/system.rs

use crate::context::ContextBuilder;
use crate::context::ExorunCtx;
use wasmtime::component::Linker;

#[derive(Debug, Clone)]
pub enum Error {
    Linker(String),
    Config(String),
}

pub trait SystemComponent: Send + Sync + 'static {
    fn install(&self, linker: &mut Linker<ExorunCtx>) -> Result<(), Error>;
    fn configure(&self, builder: &mut ContextBuilder) -> Result<(), Error>;
}
```

## Instance Handle

Create `crates/exorun/src/instance.rs`. We need a mechanism to manipulate a running instance from the outside, specifically to allow one local instance to call another. This requires a handle that wraps the Wasmtime `Store` and `Instance` in a thread-safe container.

We use `Arc<tokio::sync::Mutex<...>>` to protect the store, as Wasmtime stores are not thread-safe. Define an `InstanceHandle` struct. Provide a helper method `exec` that handles the locking ceremony, allowing the caller to provide a closure that operates on the `StoreContextMut` and `Instance`. This encapsulates the complexity of async locking and context projection.

```rust
// crates/exorun/src/instance.rs

use std::sync::Arc;
use tokio::sync::Mutex;
use wasmtime::{Store, component::Instance};
use crate::context::ExorunCtx;

#[derive(Clone)]
pub struct InstanceHandle {
    pub(crate) inner: Arc<Mutex<State>>,
}

pub(crate) struct State {
    pub store: Store<ExorunCtx>,
    pub instance: Instance,
}

impl InstanceHandle {
    pub async fn exec<F, R>(&self, f: F) -> anyhow::Result<R> 
    where F: FnOnce(&mut Store<ExorunCtx>, &Instance) -> R 
    {
        let mut guard = self.inner.lock().await;
        Ok(f(&mut guard.store, &guard.instance))
    }
}
```

## Runtime Registry

Create `crates/exorun/src/runtime.rs`. This is the central registry for the application lifecycle. It manages compiled components (Apps), connection protocols (Peers), and active executions (Instances). To ensure high performance under load, we reject the coarse-grained locking of `Arc<Mutex<HashMap>>`.

Use `dashmap::DashMap` for the internal storage of apps, peers, and instances. This allows concurrent reads and writes to different segments of the registry without contention. The `Runtime` struct should hold an `Engine` and these maps. It provides methods `register_app` and `add_peer` which generate unique IDs using `AtomicU64` and insert the resources into the maps.

```rust
// crates/exorun/src/runtime.rs

use dashmap::DashMap;
use std::sync::Arc;
use wasmtime::Engine;
use wasmtime::component::Component;
use crate::transport::Transport;

// Strong typedefs for safety
#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub struct AppId(pub u64);

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub struct PeerId(pub u64);

pub struct Runtime {
    pub(crate) engine: Engine,
    pub(crate) apps: DashMap<AppId, Component>,
    pub(crate) peers: DashMap<PeerId, Arc<dyn Transport>>,
    // ...
}
```

## Binder Extensions

Modify `crates/exorun/src/bind.rs` to support local linking. The current binder only creates stubs for remote RPC. We need a method `link_local_interface` that connects a required interface on one component directly to the exported functions of another local `InstanceHandle`.

This function iterates through the `Ledger`. For each method in the interface, it defines a host function in the `Linker`. This host function captures the `InstanceHandle` of the target. When called, it locks the target's store via `exec` and invokes the target function directly. This bypasses the serialization overhead of `neorpc`, passing `Val` types directly between stores. Use `let else` to check for the existence of functions in the ledger, returning early errors if the contract is violated.

```rust
// crates/exorun/src/bind.rs

use crate::instance::InstanceHandle;

pub fn link_local_interface(
    linker: &mut Linker<ExorunCtx>,
    ledger: &Ledger,
    interface: &str,
    target: InstanceHandle,
) -> Result<()> {
    let Some(schema) = ledger.interfaces.get(interface) else {
        return Err(Error::InterfaceNotFound(interface.into()));
    };

    for (method, sig) in &schema.funcs {
        // ... generate host closure that calls target.exec()
    }
    Ok(())
}
```

## Instance Builder

Create `crates/exorun/src/builder.rs`. This provides a fluent API for composing an instance. The builder collects a list of `Linkable` targets—whether they are Systems, Local Instances, or Remote Targets—and wires them up sequentially.

Define an enum `Linkable` that encapsulates the three linking strategies. The `instantiate` method constructs the `Linker` and `ContextBuilder`. It iterates over the registered links. If a link is a `System`, it calls `install` and `configure`. If it is `Remote`, it delegates to `Binder::link_remote_interface`. If `Local`, it delegates to `Binder::link_local_interface`. Finally, it creates the `Store` and instantiates the component, returning the `InstanceHandle`.

```rust
// crates/exorun/src/builder.rs

use crate::system::SystemComponent;
use crate::bind::{Binder, RemoteTarget};
use crate::instance::InstanceHandle;

pub enum Linkable {
    System(Box<dyn SystemComponent>),
    Local(InstanceHandle),
    Remote(RemoteTarget),
}

pub struct InstanceBuilder<'a> {
    runtime: &'a Runtime,
    app_id: AppId,
    links: HashMap<String, Linkable>,
    // ...
}

// impl InstanceBuilder...
```

## Build and Test Scripts

We must update the auxiliary scripts to target the correct directory structure. Modify `scripts/build_apps.sh` to ensure it builds the fixtures required for testing. We create a new script `scripts/test_exorun.sh` that orchestrates the build and test process.

Update `scripts/build_apps.sh`:

```bash
#!/bin/bash
set -e

ROOT_DIR=$(pwd)
# Update target directory to exorun
FIXTURES_DIR="$ROOT_DIR/crates/exorun/tests/fixtures"

mkdir -p "$FIXTURES_DIR"

build_app() {
    APP_NAME=$1
    echo "Building $APP_NAME..."
    cd "$ROOT_DIR/apps/$APP_NAME"
    cargo build --release --target wasm32-wasip2
    cp "target/wasm32-wasip2/release/$APP_NAME.wasm" "$FIXTURES_DIR/$APP_NAME.wasm"
}

for app_dir in "$ROOT_DIR/apps"/*/ ; do
    if [ -d "$app_dir" ]; then
        app_name=$(basename "$app_dir")
        build_app "$app_name"
    fi
done
```

Create `scripts/test_exorun.sh`:

```bash
#!/bin/bash
set -e

ROOT_DIR=$(pwd)

# Ensure apps are built and fixtures are ready
"$ROOT_DIR/scripts/build_apps.sh"

echo "Running exorun integration suite..."
cd "$ROOT_DIR/crates/exorun"
# Run the specific integration suite with output visibility
cargo test --test integration_suite -- --nocapture
```

## Porting the Integration Suite

We must transplant the verification logic from the reference implementation at `tmp/isorun/tests/integration_suite.rs` into our new home at `crates/exorun/tests/integration_suite.rs`. The goal is not a blind copy-paste, but a translation that aligns with our refined `SystemComponent` trait and strict error handling. The reference tests cover the critical path: runtime creation, app registration, peer management, system integration (logging), local app-to-app linking, stateful systems (KV store), and remote execution.

When adapting the `SysLogger` and `InMemoryKv` mocks, you must implement the new `configure` method required by the `SystemComponent` trait. In the reference, `install` did heavy lifting; here, we separate concerns. `install` binds the WIT interface to the linker, while `configure` sets up any necessary context state. Since our simple mocks rely on captured `Arc<Mutex<..>>` state rather than WASI context, the `configure` implementation for these mocks will remain empty. Ensure that `MockTransport` is updated to match the `exorun` definition of `Transport`, specifically regarding error types.

```rust
// crates/exorun/tests/integration_suite.rs

// ... imports (use crate::exorun::...)

struct SysLogger {
    logs: Arc<std::sync::Mutex<Vec<String>>>,
}

impl SystemComponent for SysLogger {
    fn install(&self, linker: &mut Linker<ExorunCtx>) -> anyhow::Result<()> {
        let logs = self.logs.clone();
        // logic to func_wrap "log" ...
        Ok(())
    }

    fn configure(&self, _builder: &mut ContextBuilder) -> anyhow::Result<()> {
        // No WASI config needed for this mock
        Ok(())
    }
}

#[tokio::test]
async fn test_diamond_dependency() -> anyhow::Result<()> {
    let rt = Runtime::new()?;
    // Use DashMap for the KV store backing if verifying concurrency, 
    // though Mutex<HashMap> is fine for functional correctness here.
    let shared_kv = InMemoryKv::new(); 
    
    // ... instantiation logic verifying both instances see the same data
    Ok(())
}
```

## Incremental Verification and Invariants

Construction must proceed in strict layers, verifying the bedrock of each module before building upon it. We do not proceed to the next layer until the current layer's invariants hold under both success and failure conditions.

Begin with **Context and System**. The invariant here is that the `ContextBuilder` acts as a sealed staging area that produces an immutable `ExorunCtx` populated with the exact capabilities requested. Write a unit test that attempts to mount a file system via `WasiSystem`. Verify that a guest Wasm module can read a file that exists. Crucially, verify the failure mode: attempt to read a file outside the mounted directory or a file that does not exist. The system must return a specific WASI error code, not panic or leak host paths.

Next, verify **Instance Handles**. The invariant is mutual exclusion. Spawn two async tasks that attempt to `exec` a long-running closure on the same `InstanceHandle`. Assert that they execute sequentially, not concurrently. If the store were accessible concurrently, we would see data corruption or Wasmtime panics. Test the failure boundary by ensuring that if a closure panics or returns an error, the mutex is released and the handle remains valid (or is poisoned correctly, depending on design choice), preventing a deadlock.

```rust
#[tokio::test]
async fn test_instance_handle_mutex_invariant() {
    let handle = setup_handle().await; // helper
    let h1 = handle.clone();
    let h2 = handle.clone();

    let start = std::time::Instant::now();
    
    let t1 = tokio::spawn(async move {
        h1.exec(|_, _| std::thread::sleep(std::time::Duration::from_millis(100))).await
    });
    
    let t2 = tokio::spawn(async move {
        h2.exec(|_, _| std::thread::sleep(std::time::Duration::from_millis(100))).await
    });

    let _ = tokio::join!(t1, t2);
    // Invariant: Total time must be >= 200ms, proving serialization
    assert!(start.elapsed() >= std::time::Duration::from_millis(200));
}
```

Verify the **Runtime Registry**. The invariant is unique identification and concurrent access. Use `dashmap` to its full potential by spawning multiple threads that register apps and peers simultaneously. Assert that no IDs collide and that all registered items are retrievable. Test the boundary by attempting to retrieve a non-existent ID; it must return a specific `NotFound` error variant, not `None` or a generic failure.

Finally, verify **Binder and Builder**. The invariant is that the `Ledger` strictly enforces interface contracts. Attempt to link a local instance that is missing a required function; the Binder must reject this at link-time with `InterfaceNotFound` or `MethodNotFound`. Attempt to link a component that uses a resource type across a remote boundary; the `Ledger` validation must catch this before any networking code runs. Only after these failure modes are proven should you run the full `integration_suite` to confirm end-to-end functionality.

## Refactoring NeoRPC for Layered Isolation

We must surgically separate the framing mechanism of NeoRPC from the type-specific encoding logic of Wasmtime. Currently, `CallEncoder` accepts `Val` slices directly, tightly coupling the envelope format to the payload structure. We will sever this dependency by making the `frame` module agnostic to the data it carries, treating arguments as opaque byte slabs. This allows `neorpc` to serve as a pure protocol definition where the codec is just one of many potential payload strategies.

Update `crates/neorpc/src/frame.rs` to remove all references to `wasmtime`. The `CallEncoder` struct must accept `args_payload: &'a [u8]` in its constructor. Its `encode` method will simply inject these bytes into the `args` variant container. We assume `neopack` exposes a `raw_bytes` method to bypass encoding headers; if not, you must implement it to write bytes directly to the underlying buffer without length prefixing (as the payload itself is already an encoded structure).

```rust
// crates/neorpc/src/frame.rs

use neopack::{Encoder, Decoder};
use crate::error::Result;

pub struct CallEncoder<'a> {
    pub seq: u64,
    pub target: &'a str,
    pub method: &'a str,
    /// The pre-encoded arguments list (including list headers).
    pub args_payload: &'a [u8],
}

impl<'a> CallEncoder<'a> {
    pub fn new(seq: u64, target: &'a str, method: &'a str, args_payload: &'a [u8]) -> Self {
        Self { seq, target, method, args_payload }
    }

    pub fn encode(&self, enc: &mut Encoder) -> Result<()> {
        enc.variant_begin("Call")?;
        enc.map_begin()?;
        
        // Headers
        enc.variant_begin("seq")?; enc.u64(self.seq)?; enc.variant_end()?;
        enc.variant_begin("target")?; enc.str(self.target)?; enc.variant_end()?;
        enc.variant_begin("method")?; enc.str(self.method)?; enc.variant_end()?;
        
        // Payload injection
        enc.variant_begin("args")?;
        enc.raw_bytes(self.args_payload)?; 
        enc.variant_end()?;
        
        enc.map_end()?;
        enc.variant_end()?;
        Ok(())
    }
}
```

We must also refine the error taxonomy to support the high-fidelity reporting required for distributed authentication. In `crates/neorpc/src/error.rs`, replace the generic `ProtocolViolation` or custom placeholders in `FailureReason` with a `DomainSpecific` variant. This variant carries a numeric code and a string message, allowing future system components (like the `auth` app) to signal specific rejection reasons that the client can programmatically handle.

```rust
// crates/neorpc/src/error.rs

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureReason {
    AppTrapped,
    OutOfFuel,
    OutOfMemory,
    InstanceNotFound,
    MethodNotFound,
    BadArgumentCount,
    /// Application-specific failures (e.g., Auth failure, Domain rule violation).
    /// Tuple contains (error_code, description).
    DomainSpecific(u32, String), 
}

impl FailureReason {
    pub fn as_tag(&self) -> &'static str {
        match self {
            Self::DomainSpecific(_, _) => "Domain",
            _ => "Standard", // Simplification for brevity
        }
    }
}
```

## Async Transport and Client Correlation

We move from a synchronous, blocking request-response model to a fully asynchronous message-passing architecture. The `Transport` trait in `crates/exorun/src/transport.rs` must be redefined to reflect non-blocking I/O operations. We abandon the polling-based `try_read` in favor of standard async `recv`. This shifts the scheduling burden to the Tokio runtime, which is more efficient than a busy-loop pump.

```rust
// crates/exorun/src/transport.rs

use crate::error::Error;

#[async_trait::async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Queues a raw message for transmission.
    /// This should handle framing (length-prefixing) appropriate for the underlying stream.
    async fn send(&self, payload: &[u8]) -> Result<(), Error>;

    /// Awaits the next complete message from the peer.
    /// Returns Ok(None) if the stream is closed (EOF).
    async fn recv(&self) -> Result<Option<Vec<u8>>, Error>;
}
```

The `Client` in `crates/exorun/src/client.rs` becomes the orchestrator. It maintains a concurrent map of pending requests and spawns a background task (the "pump") to demultiplex incoming responses. We use a `DashMap` to store `oneshot` channels, keyed by sequence number.

When `Client::new` is called, it spawns the pump loop immediately. This loop acts as a dedicated consumer of the Transport's `recv` method. It is vital that the pump is robust; if it crashes, the Client is dead. Therefore, use a `loop` with comprehensive error handling that logs errors but attempts to continue unless the Transport is permanently broken.

```rust
// crates/exorun/src/client.rs

use dashmap::DashMap;
use tokio::sync::oneshot;
use std::sync::Arc;
use crate::context::ExorunCtx; // Assuming context holds sequence gen

pub struct Client {
    transport: Arc<dyn Transport>,
    /// Map of Seq -> Response Channel
    pending: Arc<DashMap<u64, oneshot::Sender<Result<Vec<Val>>>>>,
    seq_gen: Arc<std::sync::atomic::AtomicU64>,
}

impl Client {
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        let pending = Arc::new(DashMap::new());
        let client = Self {
            transport: transport.clone(),
            pending: pending.clone(),
            seq_gen: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        };
        
        // Spawn the pump immediately
        tokio::spawn(async move {
            loop {
                match transport.recv().await {
                    Ok(Some(msg)) => {
                        // Decode just the sequence to route the message
                        if let Ok(seq) = neorpc::decode_seq(&msg) {
                            if let Some((_, tx)) = pending.remove(&seq) {
                                // We delegate full decoding to the waiting caller
                                // to offload work from the pump loop.
                                // We send the raw bytes; the caller uses ReplyDecoder.
                                let _ = tx.send(Ok(Client::decode_reply_payload(msg)));
                            }
                        }
                    }
                    Ok(None) => break, // Stream closed
                    Err(e) => {
                        eprintln!("Transport error in pump: {}", e);
                        // In a production system, we might break or backoff here
                        break; 
                    }
                }
            }
        });

        client
    }

    pub async fn call(&self, target: &str, method: &str, args: &[Val]) -> Result<Vec<Val>> {
        let seq = self.seq_gen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.insert(seq, tx);
        
        // 1. Encode Args via Codec (produces Vec<u8>)
        let args_bytes = neorpc::codec::encode_vals_to_bytes(args)?;

        // 2. Encode Frame via Framing (injects args_bytes)
        let frame = neorpc::CallEncoder::new(seq, target, method, &args_bytes).to_bytes()?;
        
        // 3. Send
        if let Err(e) = self.transport.send(&frame).await {
            self.pending.remove(&seq);
            return Err(e.into());
        }

        // 4. Await with Timeout
        // The pump will complete 'tx' when a matching reply arrives.
        let response_bytes = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Client("Channel closed".into()))??;

        // 5. Decode Result
        // (Implementation detail: decode_reply_payload uses ReplyDecoder)
        Ok(response_bytes)
    }
}
```

## Verification Strategy

You must verify the system boundaries strictly before integration.

**1. Frame Isolation Test:**
In `neorpc/tests.rs`, create a test that instantiates `CallEncoder` with a dummy byte slice `&[0xCA, 0xFE, 0xBA, 0xBE]` as `args_payload`. Verify that the resulting encoded buffer contains those exact bytes at the expected position, untouched by the encoder. This proves the decoupling of framing from codec.

**2. Transport Mocking:**
In `exorun/tests`, implement a `DuplexChannelTransport` using `tokio::sync::mpsc`. This mocks a real network connection.
```rust
struct DuplexChannelTransport {
    tx: mpsc::Sender<Vec<u8>>,
    rx: tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>,
}
```
This allows you to simulate network latency, out-of-order delivery, and stream termination without spinning up TCP sockets.

**3. Concurrent Correlation Test:**
Instantiate a `Client` connected to a `DuplexChannelTransport` (the "Server" side). Spawn 10 concurrent tasks, each making a `call` with a unique ID. On the Server side, read the requests, shuffle them, and send replies back in random order. Assert that every client task receives exactly the reply corresponding to its request. This proves the `DashMap` correlation logic and the `pump` loop's stability.

**4. Failure Fidelity:**
Simulate a `DomainSpecific` failure from the "Server". Ensure the `Client` propagates this error all the way up to the `Result` returned by `call`. The test passes only if the error code and message match exactly.

## Final Review

1.  **Safety:** We use `DashMap` to avoid deadlocks in the registry and `Arc<Mutex>` only where strictly necessary for the single-threaded Wasmtime store.
2.  **Style:** We enforce flat structures using guard clauses (`let else`). We avoid import aliasing to keep error origins distinct.
3.  **Completeness:** We handle all three linking types (System, Local, Remote) via a unified Builder API.
4.  **Verification:** We rely on the integration test suite, driven by the updated scripts, to confirm the behavior of the assembled system.
