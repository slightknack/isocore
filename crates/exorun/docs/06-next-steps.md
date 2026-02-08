---
origin: handwritten
date: 2026-01-20
---

# Peer Transport System: Scope Analysis and Next Steps

This document analyzes the current peer/transport implementation, identifies issues, and proposes a design for multi-transport peer connections with reconnection support.

## Current Scope

### In Scope (Currently Implemented)

1. **Transport Trait** (`transport.rs`)
   - Async message-passing interface
   - `send(&[u8])` and `recv() -> Option<Vec<u8>>` methods
   - Object-safe (`Arc<dyn Transport>`)

2. **Peer** (`peer.rs`)
   - RPC call/response correlation via sequence numbers
   - Background pump task for demultiplexing responses
   - `DashMap<u64, PendingResponse>` for concurrent request tracking

3. **Runtime Peer Registry** (`runtime.rs`)
   - `add_peer()` returns `PeerId`
   - `get_peer()` retrieves `Arc<Peer>` by ID
   - Peers stored in `DashMap<PeerId, Arc<Peer>>`

4. **Binder Integration** (`bind.rs`)
   - `peer_interface()` links imports to remote targets
   - Resolves `PeerId` to `Peer` at call time via Runtime
   - Enables transparent peer replacement (in theory)

### Out of Scope (Not Yet Implemented)

1. **Multi-Transport Support**
   - No way to connect via different protocols (QUIC, WebSocket, HTTPS)
   - No transport abstraction for protocol-specific framing

2. **Reconnection**
   - No API to replace transport on disconnect
   - No health monitoring or automatic reconnection
   - Pump task failure is terminal

3. **Peer Lifecycle Management**
   - No graceful shutdown
   - No way to remove peers from registry
   - Pump task spawned without cancellation handle

4. **Configuration**
   - Hardcoded 30-second timeout
   - No backpressure limits on pending requests
   - No configurable retry policies

---

## Feature Analysis

### 1. Transport Trait

**Status**: Good foundation, minor improvements needed.

**Current Design**:
```rust
#[async_trait::async_trait]
pub trait Transport: Send + Sync + 'static {
    async fn send(&self, payload: &[u8]) -> Result<()>;
    async fn recv(&self) -> Result<Option<Vec<u8>>>;
}
```

**Assessment**: The trait is clean and minimal. It correctly abstracts message framing from the Peer. The byte-oriented design is correct.

**Issue**: No method to check connection health or close gracefully.

**Recommendation**: Add optional `close()` method with default implementation:
```rust
async fn close(&self) -> Result<()> { Ok(()) }
```

---

### 2. Peer Pump Task

**Status**: Has bugs and design issues.

**Current Design**:
```rust
// In Peer::new()
tokio::spawn(async move {
    let error = loop {
        match pump_transport.recv().await {
            Ok(Some(msg)) => { /* handle */ }
            Ok(None) => { break Error::...; }
            Err(e) => { break Error::...; }
        }
    };
    Self::notify_all_pending(&pump_pending, error);
});
```

**Issues**:

1. **No JoinHandle stored**: The spawned task cannot be cancelled or awaited. If the Peer is dropped, the pump task continues running until it errors.

2. **No shutdown signal**: There's no way to gracefully stop the pump. It only exits on transport error or EOF.

3. **Error handling logs to stderr**: Production systems need structured logging or error propagation to a health monitor.

4. **Pump failure is silent and terminal**: Callers have no way to know the peer is dead until they try to make a call.

**Bug**: If the pump exits, `pending` responses are notified with the error, but future calls to `prepare_call()` will still insert into `pending` and wait forever (until timeout). The peer appears alive but is actually dead.

---

### 3. Timeout Handling

**Status**: Works but not configurable.

**Current Design**:
```rust
match tokio::time::timeout(Duration::from_secs(30), rx).await {
    Ok(Ok(result)) => result,
    Ok(Err(_)) => { /* channel closed */ }
    Err(_) => { /* timeout */ }
}
```

**Issues**:

1. **Hardcoded 30 seconds**: Not suitable for all use cases.
2. **No per-call timeout override**: All calls use the same timeout.

---

### 4. Peer State

**Status**: Missing entirely.

The Peer has no concept of state (Connected, Disconnected, Reconnecting). Code that uses a peer cannot check if it's healthy before making a call.

---

### 5. Transport Replacement

**Status**: Partially designed, not implemented.

The comment in `runtime.rs` notes:
> The RPC stubs resolve PeerId → Peer on every call via `Runtime::get_peer`, which would theoretically enable swapping transports by replacing the peer at the same id, though we don't currently have API for this.

The Binder resolves peers at call time, which is the right architecture. But:
- There's no `Runtime::replace_peer()` method
- Replacing a peer would orphan the old pump task
- Pending requests on the old peer would be lost

---

## Proposed Design

### Goals

1. Connect to peers over multiple channel types (QUIC, HTTPS, WebSocket)
2. Replace transport on disconnect/reconnect without losing peer identity
3. Maintain pending request correlation across transport changes
4. Provide clear peer lifecycle states
5. Enable configurable timeouts and backpressure

### Non-Goals (This Phase)

1. Automatic reconnection with backoff (future work)
2. Connection pooling (future work)
3. Stream/subscription support (future work)

---

## Ideal API Design

### Transport Factory

Instead of creating peers with a single transport, use a factory pattern:

```rust
/// Creates transports for a specific peer.
/// Allows reconnection via different protocols.
#[async_trait::async_trait]
pub trait TransportFactory: Send + Sync + 'static {
    /// Attempt to establish a connection.
    async fn connect(&self) -> transport::Result<Box<dyn Transport>>;
    
    /// Human-readable description for logging.
    fn describe(&self) -> String;
}
```

### Peer Configuration

```rust
/// Configuration for peer behavior.
#[derive(Clone, Debug)]
pub struct PeerConfig {
    /// Default timeout for RPC calls.
    pub call_timeout: Duration,
    /// Maximum number of pending requests (backpressure).
    pub max_pending: usize,
}

impl Default for PeerConfig {
    fn default() -> Self {
        Self {
            call_timeout: Duration::from_secs(30),
            max_pending: 1000,
        }
    }
}
```

### Peer State

```rust
/// Observable state of a peer connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerState {
    /// Transport is connected and pump is running.
    Connected,
    /// Transport disconnected, awaiting reconnection.
    Disconnected,
    /// Peer has been shut down and cannot be used.
    Shutdown,
}
```

### Peer API

```rust
impl Peer {
    /// Creates a new peer with initial transport.
    pub fn new(
        name: impl Into<String>,
        transport: Box<dyn Transport>,
        config: PeerConfig,
    ) -> Self;
    
    /// Returns the current connection state.
    pub fn state(&self) -> PeerState;
    
    /// Replaces the transport, restarting the pump.
    /// Pending requests are preserved and will use the new transport.
    /// Returns error if peer is shutdown.
    pub async fn reconnect(&self, transport: Box<dyn Transport>) -> Result<()>;
    
    /// Gracefully shuts down the peer.
    /// Notifies all pending requests and stops the pump.
    pub async fn shutdown(&self);
    
    /// Makes an RPC call with the default timeout.
    pub async fn call(
        &self,
        target: &str,
        method: &str,
        args: &[Val],
        result_types: Vec<Type>,
    ) -> Result<Vec<Val>>;
    
    /// Makes an RPC call with a custom timeout.
    pub async fn call_with_timeout(
        &self,
        target: &str,
        method: &str,
        args: &[Val],
        result_types: Vec<Type>,
        timeout: Duration,
    ) -> Result<Vec<Val>>;
}
```

### Runtime API Extensions

```rust
impl Runtime {
    /// Registers a peer and returns its ID.
    pub fn add_peer(&self, peer: Arc<Peer>) -> PeerId;
    
    /// Retrieves a peer by ID.
    pub fn get_peer(&self, id: PeerId) -> Result<Arc<Peer>>;
    
    /// Removes a peer from the registry.
    /// Does NOT shutdown the peer - caller must do that separately.
    pub fn remove_peer(&self, id: PeerId) -> Result<Arc<Peer>>;
    
    /// Replaces a peer at an existing ID.
    /// The old peer is returned (caller should shutdown if desired).
    /// Useful for reconnection with identity preservation.
    pub fn replace_peer(&self, id: PeerId, peer: Arc<Peer>) -> Result<Arc<Peer>>;
}
```

---

## Usage Examples

### Basic Usage

```rust
let transport = QuicTransport::connect("peer.example.com:4433").await?;
let peer = Arc::new(Peer::new("alice", transport, PeerConfig::default()));
let peer_id = runtime.add_peer(peer);

// Use the peer
let result = runtime.get_peer(peer_id)?
    .call("service", "method", &args, result_types)
    .await?;
```

### Reconnection on Different Protocol

```rust
// Initial connection via QUIC
let quic = QuicTransport::connect("peer.example.com:4433").await?;
let peer = Arc::new(Peer::new("alice", quic, PeerConfig::default()));
let peer_id = runtime.add_peer(peer.clone());

// ... later, QUIC fails ...

// Reconnect via WebSocket
let ws = WebSocketTransport::connect("wss://peer.example.com/rpc").await?;
peer.reconnect(ws).await?;

// Calls continue working with same peer_id
let result = runtime.get_peer(peer_id)?
    .call("service", "method", &args, result_types)
    .await?;
```

### Graceful Shutdown

```rust
let peer = runtime.remove_peer(peer_id)?;
peer.shutdown().await;
```

### Custom Timeout

```rust
let result = peer
    .call_with_timeout("service", "slow_method", &args, types, Duration::from_secs(120))
    .await?;
```

---

## Implementation Plan

### Phase 1: Peer Lifecycle (This PR)

1. Add `PeerState` enum
2. Add `PeerConfig` struct
3. Store `JoinHandle` for pump task
4. Implement `shutdown()` method
5. Add state tracking with `AtomicU8` or similar
6. Fix the "zombie peer" bug where calls hang after pump death

### Phase 2: Reconnection Support (This PR)

1. Implement `reconnect()` method
2. Add transport replacement logic
3. Ensure pending requests survive reconnection
4. Add `Runtime::replace_peer()` if needed

### Phase 3: Backpressure (This PR)

1. Add `max_pending` to config
2. Return error when limit exceeded
3. Consider semaphore-based approach

### Phase 4: Transport Factories (Future)

1. Define `TransportFactory` trait
2. Implement for QUIC, WebSocket, HTTPS
3. Add automatic reconnection with configurable backoff

---

## Test Cases

### Lifecycle Tests

1. `test_peer_initial_state_is_connected` - New peer starts Connected
2. `test_peer_state_after_transport_close` - State becomes Disconnected
3. `test_peer_state_after_shutdown` - State becomes Shutdown
4. `test_shutdown_notifies_pending_requests` - All pending get error
5. `test_shutdown_is_idempotent` - Can call multiple times safely

### Reconnection Tests

1. `test_reconnect_replaces_transport` - New transport is used
2. `test_reconnect_preserves_pending_requests` - In-flight calls complete
3. `test_reconnect_resets_state_to_connected` - State updates
4. `test_reconnect_on_shutdown_peer_fails` - Returns error
5. `test_calls_during_reconnect` - Behavior is well-defined

### Configuration Tests

1. `test_custom_timeout_respected` - Call times out at configured duration
2. `test_default_timeout_is_30_seconds` - Backward compatible
3. `test_max_pending_rejects_excess` - Returns backpressure error

### Zombie Peer Bug Tests

1. `test_call_after_pump_death_returns_error` - Not timeout
2. `test_pump_death_updates_state` - State becomes Disconnected

### Concurrent Tests

1. `test_concurrent_calls_during_reconnect` - No lost requests
2. `test_concurrent_reconnect_attempts` - Only one succeeds
3. `test_shutdown_during_active_calls` - Clean error propagation

---

## Error Handling

### New Error Variants

```rust
#[derive(Debug, Clone)]
pub enum Error {
    // Existing...
    Transport(transport::Error),
    NeoRpc(neorpc::Error),
    NeoPack(neopack::Error),
    Remote(FailureReason),
    Timeout,
    ChannelClosed,
    SequenceMismatch { expected: u64, received: u64 },
    
    // New...
    /// Peer is disconnected and cannot process calls.
    Disconnected,
    /// Peer has been shut down.
    Shutdown,
    /// Too many pending requests (backpressure).
    TooManyPendingRequests { limit: usize },
    /// Reconnection failed.
    ReconnectFailed(String),
}
```

---

## Migration Path

The API changes are backward compatible:

1. `Peer::new()` gains a `config` parameter with `Default` impl
2. Existing code works unchanged with default config
3. New methods are additive
4. No breaking changes to Transport trait

---

## Open Questions

1. **Should `reconnect()` drain pending requests or preserve them?**
   - Proposed: Preserve them. The sequence numbers are still valid for the new transport.
   - Risk: If the remote peer restarted, it won't recognize old sequence numbers.

2. **Should we support "hot" reconnection (new transport before old dies)?**
   - Proposed: No. Reconnect requires the peer to be in Disconnected state.
   - Alternative: Allow preemptive reconnection for zero-downtime scenarios.

3. **What happens to in-flight sends during reconnection?**
   - Proposed: They fail with `Disconnected` error. Caller can retry.

4. **Should we add a health check / ping mechanism?**
   - Deferred to future work. Can be built on top of RPC calls.
