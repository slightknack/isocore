//! WASI filesystem system component

use anyhow::Result;
use wasmtime::component::Linker;

use crate::context::ContextBuilder;
use crate::context::IsorunCtx;
use crate::traits::SystemComponent;

/// A granular system that ONLY provides WASI Filesystem capabilities.
pub struct WasiDir {
    pub host_path: String,
    pub mount_path: String,
}

impl WasiDir {
    /// Create a new WASI filesystem component with the given paths.
    pub fn new(host_path: &str, mount_path: &str) -> Self {
        Self {
            host_path: host_path.to_string(),
            mount_path: mount_path.to_string(),
        }
    }
}

impl SystemComponent for WasiDir {
    fn install(&self, linker: &mut Linker<IsorunCtx>) -> Result<()> {
        // Only link FS types and preopens. No network, no clocks, no random.
        wasmtime_wasi::p2::add_to_linker_async(linker)?;
        Ok(())
    }

    fn configure(&self, builder: &mut ContextBuilder) -> Result<()> {
        // Use the generic builder to inject the specific preopen
        use wasmtime_wasi::DirPerms;
        use wasmtime_wasi::FilePerms;
        
        builder.wasi.preopened_dir(
            &self.host_path, 
            &self.mount_path,
            DirPerms::all(),
            FilePerms::all()
        )?;
        Ok(())
    }
}
