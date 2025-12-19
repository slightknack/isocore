//! # Instance builder for local components
//!
//! Provides a fluent API for composing an instance with various linking strategies.

use std::sync::Arc;

use wasmtime::component::Linker;
use wasmtime::Store;

use crate::bind;
use crate::bind::Binder;
use crate::context::ContextBuilder;
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
    Linker(wasmtime::Error),
    Instantiate(wasmtime::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(e) => write!(f, "Runtime error: {}", e),
            Self::Host(e) => write!(f, "System error: {}", e),
            Self::Bind(e) => write!(f, "Bind error: {}", e),
            Self::Linker(e) => write!(f, "Linker error: {}", e),
            Self::Instantiate(e) => write!(f, "Instantiate error: {}", e),
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

pub type Result<T> = std::result::Result<T, Error>;

/// Linking strategy for an interface.
pub enum Link {
    System { interface: String, instance: HostInstance  },
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

    // TODO: remove?
    // pub fn context(mut self, f: impl FnOnce(&mut ContextBuilder)) -> Self {
    //     f(&mut self.context_builder);
    //     self
    // }

    pub async fn build(mut self) -> Result<InstanceId> {
        let component = self.runtime.get_component(self.component_id)?;
        let ledger = crate::ledger::Ledger::from_component(&component)
            .map_err(|e| Error::Linker(wasmtime::Error::msg(e.to_string())))?;

        let mut linker = Linker::new(self.runtime.engine());

        // Process links (consuming them to transfer ownership)
        for link in self.links {
            match link {
                Link::System { interface: _, instance: target } => {
                    target.link(&mut linker, &mut self.context_builder)?;
                }
                Link::Local { interface, instance: target } => {
                    Binder::local_interface(&mut linker, &ledger, &interface, target)?;
                }
                Link::Remote { interface, instance: target } => {
                    Binder::peer_interface(&mut linker, &ledger, &interface, target)?;
                }
            }
        }

        let ctx = self.context_builder.build(Arc::clone(&self.runtime));
        let mut store = Store::new(self.runtime.engine(), ctx);

        let instance = linker
            .instantiate_async(&mut store, &component)
            .await
            .map_err(Error::Instantiate)?;

        let state = InstanceState {
            component_id: self.component_id,
            store,
            instance,
            component,
        };

        let instance_id = self.runtime.add_instance(state);
        Ok(instance_id)
    }
}
