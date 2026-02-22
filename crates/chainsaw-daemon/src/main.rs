use std::collections::HashMap;
use std::{error::Error, future::pending};
use config::Config;
use chainsaw_core::iommu::Device;
use chainsaw_core::{gpu, iommu};
use tokio::sync::RwLock;
use zbus::{connection, fdo, interface};

use chainsaw_ebpf_loader::EbpfBlocker;

struct Daemon {
    current_mode: RwLock<String>,
    gpu_list: HashMap<String, gpu::Gpu>,
    // Cached PCI devices.
    _pci_devices: HashMap<String, Device>,
    ebpf_blocker: tokio::sync::Mutex<EbpfBlocker>,
}
impl Daemon {
    pub fn new(initial_mode: String) -> Result<Self, Box<dyn std::error::Error>> {
        let pci_devices = iommu::read_pci_devices()?;
        let gpu_list = gpu::list_gpu(&pci_devices)?;
        let ebpf_blocker = EbpfBlocker::new()?;

        Ok(Self {
            current_mode: RwLock::new(initial_mode),
            _pci_devices: pci_devices,
            gpu_list,
            ebpf_blocker: tokio::sync::Mutex::new(ebpf_blocker),
        })
    }
    
    fn get_current_hardware_mode(&self) -> Result<String, Box<dyn std::error::Error>> {
        // eBPF starts with an empty block list, so default mode is hybrid.
        Ok("hybrid".to_string())
    }
    
    fn save_mode_to_config(mode: &str) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = "/etc/chainsaw.toml";
        let config_content = format!(
            r#"# Chainsaw Daemon Configuration
# This file was automatically generated

# GPU Mode: "integrated", "hybrid"
mode = "{}"
"#,
            mode
        );
        std::fs::write(config_path, config_content)?;
        Ok(())
    }
}
#[interface(name = "com.chainsaw.daemon")]
impl Daemon {
    /// Set the GPU mode.
    ///
    /// "integrated", "hybrid".
    async fn set_mode(&self, mode: String) -> fdo::Result<String> {
        let mut current_mode_lock = self.current_mode.write().await;
        match mode.as_str() {
            "integrated" => {
                for gpu in self.gpu_list.values() {
                    // Skip the boot GPU.
                    if !gpu.is_default() {
                        // Block DRM nodes.
                        if let Some(id_str) = gpu.render_node().strip_prefix("/dev/dri/renderD")
                            && let Ok(id) = id_str.parse::<u32>() {
                                let mut blocker = self.ebpf_blocker.lock().await;
                                let _ = blocker.block_id(id);
                            }
                        if let Some(id_str) = gpu.card_node().strip_prefix("/dev/dri/card")
                            && let Ok(id) = id_str.parse::<u32>() {
                                let mut blocker = self.ebpf_blocker.lock().await;
                                let _ = blocker.block_id(id);
                            }
                        
                        // Block PCI config access.
                        let mut blocker = self.ebpf_blocker.lock().await;
                        let _ = blocker.block_pci(gpu.pci_address());
                    }
                }
                *current_mode_lock = mode.clone();
                // Persist mode.
                if let Err(e) = Self::save_mode_to_config(&mode) {
                    eprintln!("Warning: Failed to save mode to config: {}", e);
                }
                Ok(format!("Set mode to {}", mode))
            }
            "hybrid" => {
                for gpu in self.gpu_list.values() {
                    // Skip the boot GPU.
                    if !gpu.is_default() {
                        // Unblock DRM nodes.
                        if let Some(id_str) = gpu.render_node().strip_prefix("/dev/dri/renderD")
                            && let Ok(id) = id_str.parse::<u32>() {
                                let mut blocker = self.ebpf_blocker.lock().await;
                                let _ = blocker.unblock_id(id);
                            }
                        if let Some(id_str) = gpu.card_node().strip_prefix("/dev/dri/card")
                            && let Ok(id) = id_str.parse::<u32>() {
                                let mut blocker = self.ebpf_blocker.lock().await;
                                let _ = blocker.unblock_id(id);
                            }
                        
                        // Unblock PCI config access.
                        let mut blocker = self.ebpf_blocker.lock().await;
                        let _ = blocker.unblock_pci(gpu.pci_address());
                    }
                }
                *current_mode_lock = mode.clone();
                // Persist mode.
                if let Err(e) = Self::save_mode_to_config(&mode) {
                    eprintln!("Warning: Failed to save mode to config: {}", e);
                }
                Ok(format!("Set mode to {}", mode))
            }
            _ => Err(fdo::Error::InvalidArgs(format!("Unknown mode={}", mode))),
        }
    }
    /// Get the current GPU mode value.
    async fn get_mode(&self) -> String {
        self.current_mode.read().await.clone()
    }
    /// List human-readable supported modes.
    async fn list_mode(&self) -> Vec<String> {
        vec![
            "integrated".to_string(),
            "hybrid".to_string(),
        ]
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config_path = "/etc/chainsaw.toml";
    
    // Create default config if missing.
    if !std::path::Path::new(config_path).exists() {
        println!("Config file not found, creating default config at {}", config_path);
        let default_config = r#"# Chainsaw Daemon Configuration
# This file was automatically generated

# GPU Mode: "integrated", "hybrid"
mode = "hybrid"
"#;
        std::fs::write(config_path, default_config)?;
    }
    
    let settings = Config::builder()
        .add_source(config::File::with_name(config_path).required(false))
        .build()?;
    
    // Read configured mode.
    let configured_mode = settings.get_string("mode").unwrap_or_else(|_| "hybrid".to_string());    
    // Initialize daemon.
    let daemon = Daemon::new(configured_mode.clone())?;
    
    // Print discovered GPUs.
    println!("Detected GPUs:");
    for gpu in daemon.gpu_list.values() {
        println!(
            "- [#{id}] {name} | pci={pci} | render={render} | default={default}",
            id = gpu.id(),
            name = gpu.name(),
            pci = gpu.pci_address(),
            render = gpu.render_node(),
            default = gpu.is_default()
        );
    }
    
    // Apply configured mode on startup.
    let hardware_mode = daemon.get_current_hardware_mode()?;
    println!("Configured mode from config: {}", configured_mode);
    println!("Current hardware mode: {}", hardware_mode);
    if hardware_mode != configured_mode {
        println!("Hardware mode doesn't match configured mode, applying configured mode {}...", configured_mode);
        daemon.set_mode(configured_mode).await?;
    } else {
        println!("Hardware mode matches configured mode: {}", configured_mode);
    }
    let _conn = connection::Builder::system()?
        .name("com.chainsaw.daemon")?
        .serve_at("/com/chainsaw/daemon", daemon)?
        .build()
        .await?;

    println!("Daemon started");

    pending::<()>().await;

    Ok(())
}
