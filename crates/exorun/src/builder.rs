//! # Instance Builder
//!
//! Provides a fluent API for composing an instance with various linking strategies.

use std::sync::Arc;

use wasmtime::component::Linker;
use wasmtime::Store;

use crate::bind::Binder;
use crate::bind::RemoteTarget;
use crate::context::ContextBuilder;
use crate::instance::InstanceHandle;
use crate::runtime::AppId;
use crate::runtime::Runtime;
use crate::system::SystemComponent;

#[derive(Debug)]
pub enum Error {
    Runtime(crate::runtime::Error),
    System(crate::system::Error),
    Bind(crate::bind::Error),
    Linker(wasmtime::Error),
    Instantiate(wasmtime::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(e) => write!(f, "Runtime error: {}", e),
            Self::System(e) => write!(f, "System error: {}", e),
            Self::Bind(e) => write!(f, "Bind error: {}", e),
            Self::Linker(e) => write!(f, "Linker error: {}", e),
            Self::Instantiate(e) => write!(f, "Instantiate error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

impl From<crate::runtime::Error> for Error {
    fn from(e: crate::runtime::Error) -> Self {
        Self::Runtime(e)
    }
}

impl From<crate::system::Error> for Error {
    fn from(e: crate::system::Error) -> Self {
        Self::System(e)
    }
}

impl From<crate::bind::Error> for Error {
    fn from(e: crate::bind::Error) -> Self {
        Self::Bind(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Linking strategy for an interface.
pub enum Linkable {
    System(Box<dyn SystemComponent>),
    Local { interface: String, target: InstanceHandle },
    Remote { interface: String, target: RemoteTarget },
}

/// Fluent builder for creating instances with configured links.
pub struct InstanceBuilder {
    runtime: Arc<Runtime>,
    app_id: AppId,
    links: Vec<Linkable>,
    context_builder: ContextBuilder,
}

impl InstanceBuilder {
    pub fn new(runtime: Arc<Runtime>, app_id: AppId) -> Self {
        Self {
            runtime,
            app_id,
            links: Vec::new(),
            context_builder: ContextBuilder::new(),
        }
    }

    pub fn link_system(mut self, system: Box<dyn SystemComponent>) -> Self {
        self.links.push(Linkable::System(system));
        self
    }

    pub fn link_local(mut self, interface: impl Into<String>, target: InstanceHandle) -> Self {
        self.links.push(Linkable::Local {
            interface: interface.into(),
            target,
        });
        self
    }

    pub fn link_remote(mut self, interface: impl Into<String>, target: RemoteTarget) -> Self {
        self.links.push(Linkable::Remote {
            interface: interface.into(),
            target,
        });
        self
    }

    pub fn context(mut self, f: impl FnOnce(&mut ContextBuilder)) -> Self {
        f(&mut self.context_builder);
        self
    }

    pub async fn instantiate(mut self) -> Result<InstanceHandle> {
        let component = self.runtime.get_app(self.app_id)?;
        let ledger = crate::ledger::Ledger::from_component(&component)
            .map_err(|e| Error::Linker(wasmtime::Error::msg(e.to_string())))?;

        let mut linker = Linker::new(self.runtime.engine());

        // Process links (consuming them to transfer ownership)
        for link in self.links {
            match link {
                Linkable::System(system) => {
                    system.install(&mut linker)?;
                    system.configure(&mut self.context_builder)?;
                }
                Linkable::Local { interface, target } => {
                    Binder::link_local_interface(&mut linker, &ledger, &interface, target.clone())?;
                }
                Linkable::Remote { interface, target } => {
                    Binder::link_remote_interface(&mut linker, &ledger, &interface, target)?;
                }
            }
        }

        let ctx = self.context_builder.build(Arc::clone(&self.runtime));
        let mut store = Store::new(self.runtime.engine(), ctx);

        let instance = linker
            .instantiate_async(&mut store, &component)
            .await
            .map_err(Error::Instantiate)?;

        Ok(InstanceHandle::new(store, instance))
    }
}
