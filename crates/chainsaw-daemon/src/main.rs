use std::collections::HashMap;
use std::{error::Error, future::pending};
use config::Config;
use chainsaw_core::iommu::Device;
use chainsaw_core::{gpu, iommu};
use log::{info, warn};
use tokio::sync::RwLock;
use zbus::{connection, fdo, interface};

use chainsaw_ebpf_loader::EbpfBlocker;

const CONFIG_PATH: &str = "/etc/chainsaw.toml";
const MODE_INTEGRATED: &str = "integrated";
const MODE_HYBRID: &str = "hybrid";
const RENDER_NODE_PREFIX: &str = "/dev/dri/renderD";
const CARD_NODE_PREFIX: &str = "/dev/dri/card";

struct Daemon {
    current_mode: RwLock<String>,
    gpu_list: HashMap<String, gpu::Gpu>,
    // Cached PCI devices.
    _pci_devices: HashMap<String, Device>,
    ebpf_blocker: tokio::sync::Mutex<EbpfBlocker>,
}

type GpuRow = (u32, String, String, String, bool, bool);

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
        Ok(MODE_HYBRID.to_string())
    }

    fn config_contents(mode: &str) -> String {
        format!(
            r#"# Chainsaw Daemon Configuration
# This file was automatically generated

# GPU Mode: \"integrated\", \"hybrid\"
mode = \"{}\"
"#,
            mode
        )
    }

    fn ensure_config_exists() -> Result<(), Box<dyn std::error::Error>> {
        if !std::path::Path::new(CONFIG_PATH).exists() {
            info!("Config file not found, creating default config at {}", CONFIG_PATH);
            std::fs::write(CONFIG_PATH, Self::config_contents(MODE_HYBRID))?;
        }
        Ok(())
    }

    fn parse_node_id(node_path: &str, prefix: &str) -> Option<u32> {
        node_path
            .strip_prefix(prefix)
            .and_then(|id_str| id_str.parse::<u32>().ok())
    }

    fn gpu_by_id(&self, id: u32) -> Option<&gpu::Gpu> {
        self.gpu_list.values().find(|gpu| gpu.id() as u32 == id)
    }

    async fn is_gpu_blocked(&self, gpu: &gpu::Gpu) -> bool {
        let mut blocker = self.ebpf_blocker.lock().await;

        if let Ok(true) = blocker.is_pci_blocked(gpu.pci_address()) {
            return true;
        }

        if let Some(render_id) = Self::parse_node_id(gpu.render_node(), RENDER_NODE_PREFIX) {
            match blocker.is_id_blocked(render_id) {
                Ok(true) => return true,
                Ok(false) => {}
                Err(err) => warn!(
                    "Failed to read render block state for {}: {}",
                    gpu.pci_address(),
                    err
                ),
            }
        }

        if let Some(card_id) = Self::parse_node_id(gpu.card_node(), CARD_NODE_PREFIX) {
            match blocker.is_id_blocked(card_id) {
                Ok(true) => return true,
                Ok(false) => {}
                Err(err) => warn!(
                    "Failed to read card block state for {}: {}",
                    gpu.pci_address(),
                    err
                ),
            }
        }

        false
    }

    async fn list_gpu_rows(&self) -> Vec<GpuRow> {
        let mut rows = Vec::with_capacity(self.gpu_list.len());
        for gpu in self.gpu_list.values() {
            let blocked = self.is_gpu_blocked(gpu).await;
            rows.push((
                gpu.id() as u32,
                gpu.name().to_string(),
                gpu.pci_address().to_string(),
                gpu.render_node().to_string(),
                gpu.is_default(),
                blocked,
            ));
        }
        rows.sort_by_key(|row| row.0);
        rows
    }

    async fn apply_gpu_block_policy(&self, gpu: &gpu::Gpu, block: bool) {
        let mut blocker = self.ebpf_blocker.lock().await;

        if let Some(id) = Self::parse_node_id(gpu.render_node(), RENDER_NODE_PREFIX) {
            let result = if block {
                blocker.block_id(id)
            } else {
                blocker.unblock_id(id)
            };
            if let Err(err) = result {
                warn!(
                    "Failed to {} render node id {} for {}: {}",
                    if block { "block" } else { "unblock" },
                    id,
                    gpu.pci_address(),
                    err
                );
            }
        }

        if let Some(id) = Self::parse_node_id(gpu.card_node(), CARD_NODE_PREFIX) {
            let result = if block {
                blocker.block_id(id)
            } else {
                blocker.unblock_id(id)
            };
            if let Err(err) = result {
                warn!(
                    "Failed to {} card node id {} for {}: {}",
                    if block { "block" } else { "unblock" },
                    id,
                    gpu.pci_address(),
                    err
                );
            }
        }

        let pci_result = if block {
            blocker.block_pci(gpu.pci_address())
        } else {
            blocker.unblock_pci(gpu.pci_address())
        };
        if let Err(err) = pci_result {
            warn!(
                "Failed to {} PCI access for {}: {}",
                if block { "block" } else { "unblock" },
                gpu.pci_address(),
                err
            );
        }
    }
    
    fn save_mode_to_config(mode: &str) -> Result<(), Box<dyn std::error::Error>> {
        std::fs::write(CONFIG_PATH, Self::config_contents(mode))?;
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
        let block_non_boot_gpu = match mode.as_str() {
            MODE_INTEGRATED => true,
            MODE_HYBRID => false,
            _ => return Err(fdo::Error::InvalidArgs(format!("Unknown mode={}", mode))),
        };

        for gpu in self.gpu_list.values() {
            if !gpu.is_default() {
                self.apply_gpu_block_policy(gpu, block_non_boot_gpu).await;
            }
        }

        *current_mode_lock = mode.clone();
        if let Err(err) = Self::save_mode_to_config(&mode) {
            warn!("Failed to save mode to config: {}", err);
        }

        info!("Set mode to {}", mode);
        Ok(format!("Set mode to {}", mode))
    }
    /// Get the current GPU mode value.
    async fn get_mode(&self) -> String {
        self.current_mode.read().await.clone()
    }
    /// List human-readable supported modes.
    async fn list_mode(&self) -> Vec<String> {
        vec![MODE_INTEGRATED.to_string(), MODE_HYBRID.to_string()]
    }

    /// List discovered GPUs with block state.
    async fn list_gpus(&self) -> Vec<GpuRow> {
        self.list_gpu_rows().await
    }

    /// Block or unblock one GPU by ID.
    async fn set_gpu_block(&self, gpu_id: u32, blocked: bool) -> fdo::Result<String> {
        let gpu = self
            .gpu_by_id(gpu_id)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("Unknown gpu id={}", gpu_id)))?;

        self.apply_gpu_block_policy(gpu, blocked).await;
        let now_blocked = self.is_gpu_blocked(gpu).await;
        info!(
            "Set GPU {} ({}) block={} (effective={})",
            gpu_id,
            gpu.pci_address(),
            blocked,
            now_blocked
        );

        Ok(format!(
            "GPU {} block {} (effective={})",
            gpu_id,
            if blocked { "on" } else { "off" },
            now_blocked
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .try_init();

    Daemon::ensure_config_exists()?;
    let settings = Config::builder()
        .add_source(config::File::with_name(CONFIG_PATH).required(false))
        .build()?;

    let configured_mode = settings
        .get_string("mode")
        .unwrap_or_else(|_| MODE_HYBRID.to_string());
    let daemon = Daemon::new(configured_mode.clone())?;

    info!("Detected GPUs:");
    for gpu in daemon.gpu_list.values() {
        info!(
            "- [#{}] {} | pci={} | render={} | default={}",
            gpu.id(),
            gpu.name(),
            gpu.pci_address(),
            gpu.render_node(),
            gpu.is_default()
        );
    }

    let hardware_mode = daemon.get_current_hardware_mode()?;
    info!("Configured mode from config: {}", configured_mode);
    info!("Current hardware mode: {}", hardware_mode);
    if hardware_mode != configured_mode {
        info!(
            "Hardware mode doesn't match configured mode, applying configured mode {}...",
            configured_mode
        );
        daemon.set_mode(configured_mode).await?;
    } else {
        info!("Hardware mode matches configured mode: {}", configured_mode);
    }
    let conn_builder = connection::Builder::system()?;
    let _conn = conn_builder
        .name("com.chainsaw.daemon")?
        .serve_at("/com/chainsaw/daemon", daemon)?
        .build()
        .await?;

    info!("Daemon started");

    pending::<()>().await;

    Ok(())
}
