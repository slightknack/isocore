//! # Exorun: a distributed wasm component runtime
//!
//! This crate implements something like the BEAM runtime for wasm components.
//! This isn't a particularly new idea (c.f. sldty, lunatic, wasm cloud).
//!
//! The main differentiator here is that I want this crate to be:
//!
//! - Self-contained, meaning it can be deployed by copying a single static binary, or loading a simple JS file (for web runtimes).
//! - Fully dynamic, meaning the runtime exposes itself as a Wasm component, through which additional components can be registered an instantiated.
//! - Self-replicating, meaning the runtime can provide a copy of itself in a convenient format to spin up another runtime which connects back to where it originated.
//! - Peer-to-peer, meaning the runtime can connect to other semi-trusted runtimes over arbitrary networks.
//! - Secure and fault-tolerant, meaning there are bounds on memory and message size and execution budget, and strong cryptographic guarantees.
//! - Capability-safe, meaning the runtime is built from the ground up with capabilities in mind.
//!
//! I plan to run an instance of this runtime at home.isaac.sh,
//! on a beefy server in Finland I have acquired, which has a
//! 2TB nvme SSD, 64GB ECC ram, and a nice cpu + networking card.
//!
//! I am tired of AI slop, I dislike twitter, I am saddened by the dead internet.
//! I plan to host a small invite-only community on home.isaac.sh,
//! built on software implemented as Wasm components running on this runtime.
//! I'll also make a nice hosting panel so friends can
//! host their own wasm component applications on the service too.
//!
//! I plan to implement a number of nice host wasm components for making local-first software easy to write.
//! This includes:
//!
//! - Auth, for public-key cryptography (ed25519), encryption (XChaCha20Poly1305), key derivation, signatures, etc.
//! - Meta, for publishing new components, spinning up new component instances, and replicating the runtime.
//! - Core, which implements something like a hypercore signed append-only log.
//! - Sync, which implements incremental synchronization on top of core.
//! - Serve, which serves a scoped folder of local interfaces for the web.
//! - Api, which lets components handle http requests, and maybe websockets too.
//! - Writer, which bundles cores with logical clocks to order messages from multiple writers.
//! - Crdt, which implements authenticated CRDT types over bundles of writer cores.
//!
//! In addition to the standard wasi components for e.g. scoped filesystem access, time, randomness, etc.
//!
//! # Example
//!
//! ```no_run
//!
//! ```

pub mod bind;
pub mod peer;
pub mod context;
pub mod local;
pub mod ledger;
pub mod runtime;
pub mod host;
pub mod transport;

#[cfg(test)]
mod tests;
