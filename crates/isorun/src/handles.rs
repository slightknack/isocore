//! Type-safe handles for runtime resources ("Go-Style" Safety)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AppId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SystemId(pub u64);

/// Handle to a registered Transport peer (e.g. a specific open TCP connection).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PeerId(pub u64);

/// A combined address for a specific app instance living on a remote peer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RemoteAddr {
    pub peer: PeerId,
    /// The identifier the remote machine uses for the instance (e.g. "kv-primary").
    pub remote_instance: String,
}
