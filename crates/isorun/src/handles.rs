//! Type-safe handles for runtime resources.
//!
//! Instead of using raw integers or strings, isorun uses strongly-typed handles
//! to prevent accidental confusion between different resource types.
//!
//! This "Go-style" safety means you can't accidentally pass an AppId where a
//! PeerId is expected - the type system catches it at compile time.

/// Handle to a registered Wasm application (component).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AppId(pub u64);

/// Handle to a registered system component implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SystemId(pub u64);

/// Handle to a registered Transport peer.
///
/// Represents a persistent connection to another runtime (e.g. a TCP socket,
/// QUIC connection, or in-memory channel).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PeerId(pub u64);

/// A complete address for a remote instance.
///
/// Combines a peer (which runtime to talk to) with a target identifier
/// (which instance on that runtime).
///
/// # Example
///
/// ```rust,no_run
/// # use isorun::{RemoteAddr, PeerId};
/// let addr = RemoteAddr {
///     peer: peer_tcp,              // Which remote runtime
///     target_id: "kv-primary".into(),  // Which instance on that runtime
/// };
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RemoteAddr {
    /// The peer (remote runtime) to send RPCs to.
    pub peer: PeerId,
    /// The target identifier on the remote peer (e.g. "kv-primary").
    ///
    /// This maps directly to `CallFrame.target` in the RPC protocol.
    pub target_id: String,
}
