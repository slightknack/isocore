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
    fn install(&self, _linker: &mut Linker<IsorunCtx>) -> Result<()> {
        // Implementation note: Only link FS types and preopens. No network, no clocks, no random.
        // wasmtime_wasi::bindings::filesystem::types::add_to_linker(linker, |ctx| &mut ctx.wasi)?;
        // wasmtime_wasi::bindings::filesystem::preopens::add_to_linker(linker, |ctx| &mut ctx.wasi)?;
        todo!("Implement WASI filesystem bindings installation")
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
