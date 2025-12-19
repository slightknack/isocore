//! # Instance builder for local components
//!
//! Provides a fluent API for composing an instance with various linking strategies.

use std::collections::BTreeMap;
use std::sync::Arc;

use wasmtime::component::Linker;
use wasmtime::component::ComponentExportIndex;
use wasmtime::Store;

use crate::bind;
use crate::bind::Binder;
use crate::context::ContextBuilder;
use crate::ledger;
use crate::runtime;
use crate::runtime::ComponentId;
use crate::runtime::InstanceId;
use crate::runtime::InstanceState;
use crate::runtime::Runtime;
use crate::peer::PeerInstance;
use crate::host::HostInstance;
use crate::host;

#[derive(Debug)]
pub enum Error {
    Runtime(runtime::Error),
    Host(host::Error),
    Bind(bind::Error),
    Ledger(ledger::Error),
    Linker(wasmtime::Error),
    Instantiate(wasmtime::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(e) => write!(f, "runtime error: {}", e),
            Self::Host(e) => write!(f, "host error: {}", e),
            Self::Bind(e) => write!(f, "bind error: {}", e),
            Self::Ledger(e) => write!(f, "ledger error: {}", e),
            Self::Linker(e) => write!(f, "linker error: {}", e),
            Self::Instantiate(e) => write!(f, "instantiate error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

impl From<runtime::Error> for Error {
    fn from(e: runtime::Error) -> Self {
        Self::Runtime(e)
    }
}

impl From<host::Error> for Error {
    fn from(e: host::Error) -> Self {
        Self::Host(e)
    }
}

impl From<bind::Error> for Error {
    fn from(e: bind::Error) -> Self {
        Self::Bind(e)
    }
}

impl From<ledger::Error> for Error {
    fn from(e: ledger::Error) -> Self {
        Self::Ledger(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Linking strategy for an interface.
pub enum Link {
    System { interface: String, instance: HostInstance },
    Local  { interface: String, instance: InstanceId },
    Remote { interface: String, instance: PeerInstance  },
}

/// Fluent builder for creating instances with configured links.
pub struct InstanceBuilder {
    runtime: Arc<Runtime>,
    component_id: ComponentId,
    links: Vec<Link>,
    context_builder: ContextBuilder,
}

impl InstanceBuilder {
    pub fn new(runtime: Arc<Runtime>, component_id: ComponentId) -> Self {
        Self {
            runtime,
            component_id,
            links: Vec::new(),
            context_builder: ContextBuilder::new(),
        }
    }

    pub fn link_system(mut self, interface: impl Into<String>, component: HostInstance) -> Self {
        self.links.push(Link::System {
            interface: interface.into(),
            instance: component,
        });
        self
    }

    pub fn link_local(mut self, interface: impl Into<String>, target: InstanceId) -> Self {
        self.links.push(Link::Local {
            interface: interface.into(),
            instance: target,
        });
        self
    }

    pub fn link_remote(mut self, interface: impl Into<String>, target: PeerInstance) -> Self {
        self.links.push(Link::Remote {
            interface: interface.into(),
            instance: target,
        });
        self
    }

    pub async fn build(mut self) -> Result<InstanceId> {
        let component = self.runtime.get_component(self.component_id)?;
        let my_ledger = self.runtime.get_ledger(self.component_id)?;

        let mut linker = Linker::new(self.runtime.engine());

        // Process links with bidirectional validation
        for link in &self.links {
            match link {
                Link::System { interface, instance: host_instance } => {
                    // Validate that the host instance can provide this interface
                    host_instance.validate_interface(interface)?;
                    host_instance.link(&mut linker, &mut self.context_builder)?;
                }
                Link::Local { interface, instance: target_id } => {
                    // Bidirectional validation: check target exports match my imports
                    self.validate_local_link(interface, *target_id)?;
                    Binder::local_interface(&mut linker, &my_ledger, interface, *target_id)?;
                }
                Link::Remote { interface, instance: target } => {
                    // For remote links, we can only validate our import side
                    // (we don't have the remote component's ledger)
                    Binder::peer_interface(&mut linker, &my_ledger, interface, target.clone())?;
                }
            }
        }

        let ctx = self.context_builder.build(Arc::clone(&self.runtime));
        let mut store = Store::new(self.runtime.engine(), ctx);

        let instance = linker
            .instantiate_async(&mut store, &component)
            .await
            .map_err(Error::Instantiate)?;

        // Pre-compute export indices for fast runtime.call()
        let export_indices = build_export_indices(&component, &my_ledger.exports)?;

        let state = InstanceState {
            component_id: self.component_id,
            store,
            instance,
            component,
            export_indices,
        };

        let instance_id = self.runtime.add_instance(state);
        Ok(instance_id)
    }

    /// Validates that a local link is compatible: my import matches target's export.
    fn validate_local_link(&self, interface: &str, target_id: InstanceId) -> Result<()> {
        let my_ledger = self.runtime.get_ledger(self.component_id)?;
        
        // Get my import schema
        let my_import = my_ledger.imports.get(interface)
            .ok_or_else(|| ledger::Error::InvalidParameter {
                import_name: interface.to_string(),
                details: "interface not found in component imports".to_string(),
            })?;
        
        // Get target's component ID and ledger
        let target_state = self.runtime.instances
            .get(&target_id)
            .ok_or(runtime::Error::InstanceNotFound(target_id))?;
        
        let target_component_id = target_state.value().try_lock()
            .map_err(|_| runtime::Error::InstanceNotFound(target_id))?
            .component_id;
        
        let target_ledger = self.runtime.get_ledger(target_component_id)?;
        
        // Get target's export schema
        let target_export = target_ledger.exports.get(interface)
            .ok_or_else(|| ledger::Error::InvalidParameter {
                import_name: interface.to_string(),
                details: format!("target instance {} does not export interface '{}'", target_id, interface),
            })?;
        
        // Validate compatibility
        ledger::validate_compatibility(interface, my_import, target_export)?;
        
        Ok(())
    }
}

/// Builds a two-level map of export indices for all exported functions.
///
/// Uses BTreeMap for O(log n) lookup without string allocation on the hot path.
fn build_export_indices(
    component: &wasmtime::component::Component,
    exports: &std::collections::HashMap<String, ledger::InterfaceSchema>,
) -> Result<BTreeMap<String, BTreeMap<String, ComponentExportIndex>>> {
    
    let mut indices = BTreeMap::new();
    
    for (interface_name, interface_schema) in exports {
        // Get the interface export index
        let inst_idx = component
            .get_export_index(None, interface_name)
            .ok_or_else(|| ledger::Error::InvalidResult {
                import_name: interface_name.clone(),
                details: "interface not found in component exports".to_string(),
            })?;
        
        let mut func_indices = BTreeMap::new();
        for func_name in interface_schema.funcs.keys() {
            // Get the function export index within the interface
            let func_idx = component
                .get_export_index(Some(&inst_idx), func_name)
                .ok_or_else(|| ledger::Error::InvalidResult {
                    import_name: format!("{}#{}", interface_name, func_name),
                    details: "function not found in interface exports".to_string(),
                })?;
            
            func_indices.insert(func_name.clone(), func_idx);
        }
        
        indices.insert(interface_name.clone(), func_indices);
    }
    
    Ok(indices)
}
