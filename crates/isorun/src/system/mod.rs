//! Built-in system implementations and helpers

mod local_channel;
mod wasi_fs;

pub use local_channel::LocalChannelTransport;
pub use wasi_fs::WasiDir;

/// Built-in system implementations and helpers.
pub struct System;

impl System {
    /// A generic transport that just routes to another local runtime in memory.
    /// Useful for testing or thread-to-thread communication without sockets.
    pub fn local_channel() -> (LocalChannelTransport, LocalChannelTransport) {
        LocalChannelTransport::pair()
    }

    /// Provide standard WASI Filesystem.
    pub fn wasi_dir(host_path: &str, mount_path: &str) -> WasiDir {
        WasiDir::new(host_path, mount_path)
    }
}
