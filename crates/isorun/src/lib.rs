//! # isorun
//!
//! A distributed Wasmtime component runtime with first-class RPC support.
//!
//! ## Architecture
//!
//! isorun provides a clean abstraction for running WebAssembly components with
//! flexible linking strategies:
//!
//! - **Local System**: Link to Rust implementations (filesystem, database, etc.)
//! - **Local Instance**: Link to other Wasm instances in the same process
//! - **Remote Instance**: Link to instances on remote peers via RPC
//!
//! ## Core Concepts
//!
//! - **Runtime**: The global registry for apps, peers, and running instances
//! - **InstanceBuilder**: Fluent API for wiring up imports and instantiating components
//! - **Linkable**: The three linking strategies (System, LocalInstance, Remote)
//! - **Transport**: Generic interface for moving RPC bytes (TCP, QUIC, etc.)
//!
//! ## Example
//!
//! ```rust,no_run
//! use isorun::{Runtime, InstanceBuilder, Linkable};
//!
//! # async fn example() -> anyhow::Result<()> {
//! let rt = Runtime::new()?;
//!
//! // Register an app
//! let app_id = rt.register_app("my-app", &wasm_bytes).await?;
//!
//! // Instantiate with a system component linked
//! let instance = InstanceBuilder::new(&rt, app_id)
//!     .link_system("wasi:filesystem/types", MyFilesystem)
//!     .instantiate()
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## RPC Protocol
//!
//! Built on [neorpc]'s efficient binary protocol:
//! - Call: `[seq, target, method, [args...]]`
//! - Reply: `[seq, Result<[results...], FailureReason>]`
//!
//! All encoding/decoding is automatic - just link to a `Remote` and call methods normally.

pub mod builder;
pub mod context;
pub mod handles;
pub mod instance;
pub mod introspect;
pub mod linker;
pub mod rpc;
pub mod runtime;
pub mod system;
pub mod traits;

pub use builder::InstanceBuilder;
pub use builder::Linkable;
pub use context::Budget;
pub use context::ContextBuilder;
pub use context::IsorunCtx;
pub use handles::AppId;
pub use handles::PeerId;
pub use handles::RemoteAddr;
pub use handles::SystemId;
pub use instance::InstanceHandle;
pub use runtime::Runtime;
pub use traits::SystemComponent;
pub use traits::Transport;
