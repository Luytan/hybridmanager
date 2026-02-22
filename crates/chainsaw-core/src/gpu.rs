use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use crate::iommu::{self, Device};

/// Struct representing a GPU device
#[derive(Debug, Clone)]
pub struct Gpu {
    id: usize,
    name: String,
    pci: String,
    render: String,
    card: String,
    default: bool,
    slot: usize,
}

impl Gpu {
    /// Returns the PCI address of the GPU
    pub fn pci_address(&self) -> &str {
        &self.pci
    }

    /// Returns true if this GPU is the default/boot GPU
    pub fn is_default(&self) -> bool {
        self.default
    }

    /// Returns the numeric id assigned during discovery
    pub fn id(&self) -> usize {
        self.id
    }

    /// Returns the human-readable device name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the DRM render node path (e.g., /dev/dri/renderD128)
    pub fn render_node(&self) -> &str {
        &self.render
    }

    /// Returns the DRM card node path (e.g., /dev/dri/card1)
    pub fn card_node(&self) -> &str {
        &self.card
    }

    /// Returns the PCI slot number
    pub fn slot(&self) -> usize {
        self.slot
    }
}

/// Discover GPUs among PCI devices using the VGA (0x030000) class code.
pub fn list_gpu(pci_devices: &HashMap<String, Device>) -> io::Result<HashMap<String, Gpu>> {
    let mut gpu_map = HashMap::new();

    for (id, device) in pci_devices
        .values()
        .filter(|device| device.class.as_str() == "0x030000")
        .enumerate()
    {
        let boot_vga_path = Path::new("/sys/bus/pci/devices")
            .join(&device.pci_address)
            .join("boot_vga");

        let is_default = fs::read_to_string(boot_vga_path)?.trim() == "1";

        let gpu = Gpu {
            id,
            name: device.device_name.clone(),
            pci: device.pci_address.clone(),
            render: render_node_path(&device.pci_address),
            card: card_node_path(&device.pci_address),
            default: is_default,
            slot: find_pci_slot(&device.pci_address).unwrap_or(0),
        };

        gpu_map.insert(id.to_string(), gpu);
    }

    Ok(gpu_map)
}

/// Refresh GPU information for a specific PCI address
fn refresh_gpu(pci_address: &str, pci_devices: &HashMap<String, Device>) -> io::Result<Option<Gpu>> {
    let device = match pci_devices.values().find(|d| d.pci_address == pci_address) {
        Some(d) => d,
        None => return Ok(None),
    };

    if device.class.as_str() != "0x030000" {
        return Ok(None);
    }

    let boot_vga_path = Path::new("/sys/bus/pci/devices")
        .join(&device.pci_address)
        .join("boot_vga");

    let is_default = fs::read_to_string(boot_vga_path)?.trim() == "1";

    let gpu = Gpu {
        id: 0,
        name: device.device_name.clone(),
        pci: device.pci_address.clone(),
        render: render_node_path(&device.pci_address),
        card: card_node_path(&device.pci_address),
        default: is_default,
        slot: find_pci_slot(&device.pci_address).unwrap_or(0),
    };

    Ok(Some(gpu))
}

/// unbind the gpu using the pci bus then power-down the device
/// before unbinding, the function should check if the gpu is in use via fn is_sleeping
pub fn unbind_gpu(pci_address: &str, slot: usize) -> io::Result<()> {
    if !is_sleeping(pci_address)? {
        return Err(std::io::Error::other(
            "Device is not in sleep state (D3cold), cannot safely unbind",
        ));
    }

    let unbind_path = Path::new("/sys/bus/pci/devices")
        .join(pci_address)
        .join("driver")
        .join("unbind");

    fs::write(unbind_path, pci_address)?;
        
    let remove_path = Path::new("/sys/bus/pci/devices")
        .join(pci_address)
        .join("remove");

    fs::write(remove_path, "1")?;
    // Power off the GPU after unbinding
    set_gpu_power(slot, false)?;
    
    Ok(())
}
/// Re-bind the GPU to its driver.
/// Powers on the GPU, triggers a PCI rescan, and returns the updated GPU info with new render node.
pub fn bind_gpu(pci_address: &str, slot: usize) -> io::Result<Option<Gpu>> {
    // Power on the GPU before binding
    set_gpu_power(slot, true)?;

    iommu::pci_rescan()?;

    // Refresh PCI devices and get updated GPU info
    let pci_devices = iommu::read_pci_devices()?;
    refresh_gpu(pci_address, &pci_devices)
}

/// Checks the device power_state status, returns 1 if GPU is D3cold
fn is_sleeping(pci_address: &str) -> io::Result<bool> {
    let power_state_path = Path::new("/sys/bus/pci/devices")
        .join(pci_address)
        .join("power_state");

    Ok(fs::read_to_string(power_state_path)?.trim() == "D3cold")
}


fn render_node_path(pci_address: &str) -> String {
    let dri_path = format!("/dev/dri/by-path/pci-{}-render", pci_address);

    fs::read_link(&dri_path)
        .map(|render| render.to_string_lossy().replace("../", "/dev/dri/"))
        .unwrap_or_else(|_| "Error: couldn't read symlink for render node".to_string())
}

fn card_node_path(pci_address: &str) -> String {
    let dri_path = format!("/dev/dri/by-path/pci-{}-card", pci_address);

    fs::read_link(&dri_path)
        .map(|card| card.to_string_lossy().replace("../", "/dev/dri/"))
        .unwrap_or_else(|_| "Error: couldn't read symlink for card node".to_string())
}

/// Find the PCI slot number for a given PCI address
fn find_pci_slot(pci_address: &str) -> io::Result<usize> {
    let slots_dir = Path::new("/sys/bus/pci/slots");
    
    // Remove the function part (e.g., "0000:03:00.0" -> "0000:03:00")
    let pci_short = pci_address.trim_end_matches(|c: char| c == '.' || c.is_ascii_digit());
    
    // Iterate through all slot directories
    for entry in fs::read_dir(slots_dir)? {
        let entry = entry?;
        if let Ok(slot_name) = entry.file_name().into_string() {
            // Try to parse the slot number
            if let Ok(slot_num) = slot_name.parse::<usize>() {
                let address_path = entry.path().join("address");
                if let Ok(address) = fs::read_to_string(&address_path)
                    && address.trim() == pci_short
                {
                    return Ok(slot_num);
                }
            }
        }
    }
    
    Err(io::Error::new(io::ErrorKind::NotFound, "PCI slot not found"))
}


pub fn is_dgpu_bound(pci_address: &str) -> io::Result<bool> {
    let driver_path = Path::new("/sys/bus/pci/devices")
        .join(pci_address)
        .join("driver");

    Ok(driver_path.exists())
}

/// Power on or off a GPU using its PCI slot
pub fn set_gpu_power(slot: usize, power_on: bool) -> io::Result<()> {
    let power_path = Path::new("/sys/bus/pci/slots")
        .join(slot.to_string())
        .join("power");

    let value = if power_on { "1" } else { "0" };
    fs::write(power_path, value)?;
    
    Ok(())
}
