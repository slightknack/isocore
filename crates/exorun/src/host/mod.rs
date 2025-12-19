//! # Host components implemented natively
//!
//! Host components are native Rust implementations that provide functionality
//! to Wasm components through host-defined interfaces.
//!
//! Host components are organized as an exhaustive enum,
//! with each variant implemented in its own module under `src/system/`.

pub mod instance;
pub mod wasi;
pub mod logger;
pub mod kv;

pub use instance::HostInstance;
pub use wasi::Wasi;
pub use logger::Logger;
pub use kv::Kv;

#[derive(Debug)]
pub enum Error {
    Link(String),
    Wasmtime(wasmtime::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Link(msg) => write!(f, "Linker error: {}", msg),
            Self::Wasmtime(e) => write!(f, "Wasmtime error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

impl From<wasmtime::Error> for Error {
    fn from(e: wasmtime::Error) -> Self {
        Self::Wasmtime(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
