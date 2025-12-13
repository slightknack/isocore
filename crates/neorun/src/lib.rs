// This will be the main library file.
// For now, it just declares the modules we will create.

pub mod builder;
pub mod context;
pub mod handles;
pub mod instance;
pub mod runtime;
pub mod traits;
pub mod rpc;
pub mod introspect;
pub mod linker;

// Re-export the public API, following isorun's pattern.
pub use builder::{InstanceBuilder, Linkable};
pub use context::IsorunCtx;
pub use handles::{AppId, PeerId, RemoteAddr};
pub use instance::InstanceHandle;
pub use runtime::Runtime;
pub use traits::Transport;


#[cfg(test)]
mod tests;