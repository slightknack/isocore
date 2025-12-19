//! # Runtime registry of peers, components, and instances
//!
//! Central registry for peers, components, and instances.
//! Manages compiled components (Components),
//! and active executions (Instances),
//! and other connected runtimes (Peers),
//!
//! Uses DashMap for concurrent access without global locking, enabling high-throughput
//! scenarios where multiple tasks register apps or spawn instances simultaneously.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use dashmap::DashMap;
use tokio::sync::Mutex;
use wasmtime::Engine;
use wasmtime::Store;
use wasmtime::component::Component;
use wasmtime::component::Instance;
use wasmtime::component::Val;

use crate::local::InstanceBuilder;
use crate::peer::Peer;
use crate::peer::PeerInstance;
use crate::context::ExorunCtx;

/// Strong type for component identifiers.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct ComponentId(pub u64);

impl std::fmt::Display for ComponentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "component-{}", self.0)
    }
}

/// Strong type for peer identifiers.
///
/// Represents a stable logical identity for a remote peer, independent of
/// network address or transport protocol.
///
/// The RPC stubs resolve PeerId → Peer on every call via [`Runtime::get_peer`],
/// which would theoretically enable swapping transports by replacing the peer
/// at the same id, though we don't currently have API for this.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct PeerId(pub u64);

impl std::fmt::Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "peer-{}", self.0)
    }
}

impl PeerId {
    pub fn get_instance(&self, target_id: impl Into<String>) -> PeerInstance {
        PeerInstance {
            peer_id: *self,
            target_id: target_id.into(),
        }
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
    ComponentNotFound(ComponentId),
    PeerNotFound(PeerId),
    InstanceNotFound(InstanceId),
    InterfaceNotFound { interface: String },
    FunctionNotFound { interface: String, function: String },
    FunctionLookupFailed,
    Engine(wasmtime::Error),
    Component(wasmtime::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ComponentNotFound(id) => write!(f, "component not found: {}", id),
            Self::PeerNotFound(id) => write!(f, "peer not found: {}", id),
            Self::InstanceNotFound(id) => write!(f, "instance not found: {}", id),
            Self::InterfaceNotFound { interface } => write!(f, "interface '{}' not found", interface),
            Self::FunctionNotFound { interface, function } => write!(f, "function '{}' not found in interface '{}'", function, interface),
            Self::FunctionLookupFailed => write!(f, "failed to get function from instance"),
            Self::Engine(e) => write!(f, "engine error: {}", e),
            Self::Component(e) => write!(f, "component error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Internal state for a running instance.
/// The Store is !Send, so we wrap it in Arc<Mutex> for async access.
///
/// Note: This is public for advanced use cases (e.g., custom system components),
/// but most users should use `Runtime::instantiate()` instead.
pub struct InstanceState {
    pub component_id: ComponentId,
    pub store: Store<ExorunCtx>,
    pub instance: Instance,
    pub component: Component,
}

/// The central runtime for managing Wasm components and their instances.
///
/// Provides concurrent registration and lookup for:
/// - Components: Compiled Wasm components ready for instantiation
/// - Peers: Remote connections identified by logical PeerId
/// - Instances: Running component instances
pub struct Runtime {
    pub(crate) engine: Engine,
    pub(crate) peers: DashMap<PeerId, Arc<Peer>>,
    pub(crate) components: DashMap<ComponentId, Component>,
    pub(crate) instances: DashMap<InstanceId, Arc<Mutex<InstanceState>>>,
    next_peer_id: AtomicU64,
    next_component_id: AtomicU64,
    next_instance_id: AtomicU64,
}

impl Runtime {
    /// Creates a new runtime with default engine configuration.
    pub fn new() -> Result<Arc<Self>> {
        let mut config = wasmtime::Config::new();
        config.async_support(true);
        config.wasm_component_model(true);

        let engine = Engine::new(&config).map_err(Error::Engine)?;

        Ok(Arc::new(Self {
            engine,
            components: DashMap::new(),
            peers: DashMap::new(),
            instances: DashMap::new(),
            next_component_id: AtomicU64::new(1),
            next_peer_id: AtomicU64::new(1),
            next_instance_id: AtomicU64::new(1),
        }))
    }

    /// Creates a new runtime with a custom engine configuration.
    pub fn with_engine(engine: Engine) -> Arc<Self> {
        Arc::new(Self {
            engine,
            components: DashMap::new(),
            peers: DashMap::new(),
            instances: DashMap::new(),
            next_component_id: AtomicU64::new(1),
            next_peer_id: AtomicU64::new(1),
            next_instance_id: AtomicU64::new(1),
        })
    }

    /// Returns a reference to the wasmtime Engine.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Registers a compiled component and returns its unique ID.
    ///
    /// The component bytes are compiled if not already a Component.
    pub fn add_component_bytes(&self, bytes: &[u8]) -> Result<ComponentId> {
        let component = Component::new(&self.engine, bytes).map_err(Error::Component)?;
        let id = ComponentId(self.next_component_id.fetch_add(1, Ordering::Relaxed));
        self.components.insert(id, component);
        Ok(id)
    }

    /// Registers a pre-compiled component and returns its unique ID.
    pub fn add_component(&self, component: Component) -> ComponentId {
        let id = ComponentId(self.next_component_id.fetch_add(1, Ordering::Relaxed));
        self.components.insert(id, component);
        id
    }

    /// Retrieves a component by ID.
    pub fn get_component(&self, id: ComponentId) -> Result<Component> {
        self.components
            .get(&id)
            .map(|entry| entry.value().clone())
            .ok_or(Error::ComponentNotFound(id))
    }

    /// Registers an instance and returns its unique ID.
    ///
    /// This is an internal API used by the InstanceBuilder.
    /// Users should use `Runtime::instantiate()` instead.
    pub(crate) fn add_instance(&self, state: InstanceState) -> InstanceId {
        let id = InstanceId(self.next_instance_id.fetch_add(1, Ordering::Relaxed));
        self.instances.insert(id, Arc::new(Mutex::new(state)));
        id
    }

    /// Creates an instance builder for the given component.
    /// This is the primary way to instantiate components.
    pub fn instantiate(self: &Arc<Self>, component_id: ComponentId) -> InstanceBuilder {
        InstanceBuilder::new(Arc::clone(self), component_id)
    }

    /// Calls an exported function on an instance.
    pub async fn call(
        &self,
        instance_id: InstanceId,
        interface: &str,
        function: &str,
        args: &[Val],
    ) -> Result<Vec<Val>> {
        let state_arc = self.instances
            .get(&instance_id)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or(Error::InstanceNotFound(instance_id))?;

        let mut state = state_arc.lock().await;
        let InstanceState { component, instance, store, .. } = &mut *state;

        // Get export indices
        let inst_idx = component
            .get_export_index(None, interface)
            .ok_or_else(|| Error::InterfaceNotFound {
                interface: interface.to_string()
            })?;

        let func_idx = component
            .get_export_index(Some(&inst_idx), function)
            .ok_or_else(|| Error::FunctionNotFound {
                interface: interface.to_string(),
                function: function.to_string()
            })?;

        // Get function from instance
        let func = instance
            .get_func(&mut *store, &func_idx)
            .ok_or(Error::FunctionLookupFailed)?;

        // Determine result count from function type
        let func_ty = func.ty(&mut *store);
        let result_count = func_ty.results().len();
        let mut results = vec![Val::Bool(false); result_count];

        func.call_async(&mut *store, args, &mut results)
            .await
            .map_err(Error::Component)?;

        Ok(results)
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
    pub fn get_peer(&self, peer_id: PeerId) -> Result<Arc<Peer>> {
        self.peers
            .get(&peer_id)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or(Error::PeerNotFound(peer_id))
    }
}
