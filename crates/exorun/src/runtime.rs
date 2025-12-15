//! # Runtime Registry
//!
//! Central registry for the application lifecycle. Manages compiled components (Apps)
//! and active executions (Instances).
//!
//! Uses DashMap for concurrent access without global locking, enabling high-throughput
//! scenarios where multiple tasks register apps or spawn instances simultaneously.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use dashmap::DashMap;
use wasmtime::Engine;
use wasmtime::component::Component;

use crate::peer::Peer;
use crate::instance::InstanceHandle;

/// Strong type for application identifiers.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct AppId(pub u64);

impl std::fmt::Display for AppId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "app-{}", self.0)
    }
}

/// Strong type for peer identifiers.
/// Represents a stable logical identity for a remote peer, independent of
/// network address or transport protocol.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct PeerId(pub u64);

impl std::fmt::Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "peer-{}", self.0)
    }
}

/// Strong type for instance identifiers.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct InstanceId(pub u64);

impl std::fmt::Display for InstanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "instance-{}", self.0)
    }
}

#[derive(Debug)]
pub enum Error {
    AppNotFound(AppId),
    PeerNotFound(PeerId),
    InstanceNotFound(InstanceId),
    Engine(wasmtime::Error),
    Component(wasmtime::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AppNotFound(id) => write!(f, "App not found: {}", id),
            Self::PeerNotFound(id) => write!(f, "Peer not found: {}", id),
            Self::InstanceNotFound(id) => write!(f, "Instance not found: {}", id),
            Self::Engine(e) => write!(f, "Engine error: {}", e),
            Self::Component(e) => write!(f, "Component error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// The central runtime for managing Wasm components and their instances.
///
/// Provides concurrent registration and lookup for:
/// - Apps: Compiled Wasm components ready for instantiation
/// - Peers: Remote connections identified by logical PeerId
/// - Instances: Running component instances
pub struct Runtime {
    pub(crate) engine: Engine,
    pub(crate) apps: DashMap<AppId, Component>,
    pub(crate) peers: DashMap<PeerId, Arc<Peer>>,
    pub(crate) instances: DashMap<InstanceId, InstanceHandle>,
    next_app_id: AtomicU64,
    next_peer_id: AtomicU64,
    next_instance_id: AtomicU64,
}

impl Runtime {
    /// Creates a new runtime with default engine configuration.
    pub fn new() -> Result<Self> {
        let mut config = wasmtime::Config::new();
        config.async_support(true);
        config.wasm_component_model(true);

        let engine = Engine::new(&config).map_err(Error::Engine)?;

        Ok(Self {
            engine,
            apps: DashMap::new(),
            peers: DashMap::new(),
            instances: DashMap::new(),
            next_app_id: AtomicU64::new(1),
            next_peer_id: AtomicU64::new(1),
            next_instance_id: AtomicU64::new(1),
        })
    }

    /// Creates a new runtime with a custom engine configuration.
    pub fn with_engine(engine: Engine) -> Self {
        Self {
            engine,
            apps: DashMap::new(),
            peers: DashMap::new(),
            instances: DashMap::new(),
            next_app_id: AtomicU64::new(1),
            next_peer_id: AtomicU64::new(1),
            next_instance_id: AtomicU64::new(1),
        }
    }

    /// Returns a reference to the wasmtime Engine.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Registers a compiled component and returns its unique ID.
    ///
    /// The component bytes are compiled if not already a Component.
    pub fn register_app(&self, bytes: &[u8]) -> Result<AppId> {
        let component = Component::new(&self.engine, bytes).map_err(Error::Component)?;
        let id = AppId(self.next_app_id.fetch_add(1, Ordering::Relaxed));
        self.apps.insert(id, component);
        Ok(id)
    }

    /// Registers a pre-compiled component and returns its unique ID.
    pub fn register_component(&self, component: Component) -> AppId {
        let id = AppId(self.next_app_id.fetch_add(1, Ordering::Relaxed));
        self.apps.insert(id, component);
        id
    }

    /// Registers an instance handle and returns its unique ID.
    pub(crate) fn register_instance(&self, handle: InstanceHandle) -> InstanceId {
        let id = InstanceId(self.next_instance_id.fetch_add(1, Ordering::Relaxed));
        self.instances.insert(id, handle);
        id
    }

    /// Retrieves a component by ID.
    pub fn get_app(&self, id: AppId) -> Result<Component> {
        self.apps
            .get(&id)
            .map(|entry| entry.value().clone())
            .ok_or(Error::AppNotFound(id))
    }

    /// Registers a peer with the runtime and returns its unique ID.
    /// The peer name is stored in the Peer for logging and diagnostics.
    pub fn add_peer(&self, peer: Arc<Peer>) -> PeerId {
        let id = PeerId(self.next_peer_id.fetch_add(1, Ordering::Relaxed));
        self.peers.insert(id, peer);
        id
    }

    /// Retrieves the peer handle for a given peer ID.
    /// Returns an error if the peer is not registered.
    pub fn get_peer(&self, peer_id: &PeerId) -> Result<Arc<Peer>> {
        self.peers
            .get(peer_id)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or(Error::PeerNotFound(*peer_id))
    }

    /// Retrieves an instance handle by ID.
    pub fn get_instance(&self, id: InstanceId) -> Result<InstanceHandle> {
        self.instances
            .get(&id)
            .map(|entry| entry.value().clone())
            .ok_or(Error::InstanceNotFound(id))
    }

    /// Removes an instance from the registry and initiates cleanup.
    ///
    /// This removes the instance handle from the registry. The actual cleanup of
    /// resources (WASI file handles, memory, etc.) happens when the last reference
    /// to the InstanceHandle is dropped.
    ///
    /// # Important Notes
    ///
    /// - If other code holds clones of the InstanceHandle (via `get_instance`),
    ///   cleanup will be delayed until all references are dropped.
    /// - Any ongoing async operations on the instance will continue until they
    ///   complete or the handle is dropped.
    /// - WASI resources are automatically cleaned up by the Store's Drop implementation.
    ///
    /// # Future Considerations
    ///
    /// In the future, this could be enhanced to:
    /// - Forcefully terminate any running operations
    /// - Cancel pending RPC calls
    /// - Immediately flush and close WASI file handles
    /// - Send shutdown signals to linked instances
    pub fn remove_instance(&self, id: InstanceId) -> Result<()> {
        self.instances
            .remove(&id)
            .ok_or(Error::InstanceNotFound(id))?;
        Ok(())
    }
}
