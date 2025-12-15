//! # System Component Interface
//!
//! Defines system components - native Rust implementations that provide functionality
//! to Wasm components through host-defined interfaces.
//!
//! System components are organized as an exhaustive enum, with each variant implemented
//! in its own module under `src/system/`.

pub mod target;
pub mod wasi;

pub use target::SystemTarget;
pub use wasi::WasiSystem;

#[derive(Debug)]
pub enum Error {
    Linker(String),
    Config(String),
    Wasmtime(wasmtime::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Linker(msg) => write!(f, "Linker error: {}", msg),
            Self::Config(msg) => write!(f, "Configuration error: {}", msg),
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
