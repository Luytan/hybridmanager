use aya::{Ebpf, Btf};
use aya::programs::Lsm;
use aya::maps::HashMap;

pub struct EbpfBlocker {
    ebpf: Ebpf,
}

impl EbpfBlocker {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut ebpf = Ebpf::load(aya::include_bytes_aligned!(concat!(
            env!("OUT_DIR"),
            "/bpf.o"
        )))?;

        let btf = Btf::from_sys_fs()?;
        let program: &mut Lsm = ebpf.program_mut("file_open").unwrap().try_into()?;
        program.load("file_open", &btf)?;
        program.attach()?;

        Ok(Self { ebpf })
    }

    pub fn block_id(&mut self, id: u32) -> Result<(), Box<dyn std::error::Error>> {
        let mut map: HashMap<_, u32, u8> = HashMap::try_from(self.ebpf.map_mut("BLOCKED_IDS").unwrap())?;
        map.insert(id, 1, 0)?;
        Ok(())
    }

    pub fn unblock_id(&mut self, id: u32) -> Result<(), Box<dyn std::error::Error>> {
        let mut map: HashMap<_, u32, u8> = HashMap::try_from(self.ebpf.map_mut("BLOCKED_IDS").unwrap())?;
        let _ = map.remove(&id);
        Ok(())
    }

    pub fn block_pci(&mut self, pci: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut map: HashMap<_, [u8; 16], u8> = HashMap::try_from(self.ebpf.map_mut("BLOCKED_PCI").unwrap())?;
        let mut key = [0u8; 16];
        let bytes = pci.as_bytes();
        let len = bytes.len().min(15);
        key[..len].copy_from_slice(&bytes[..len]);
        // Keep C-string termination for BPF-side lookup.
        key[len] = 0;
        map.insert(key, 1, 0)?;
        Ok(())
    }

    pub fn unblock_pci(&mut self, pci: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut map: HashMap<_, [u8; 16], u8> = HashMap::try_from(self.ebpf.map_mut("BLOCKED_PCI").unwrap())?;
        let mut key = [0u8; 16];
        let bytes = pci.as_bytes();
        let len = bytes.len().min(15);
        key[..len].copy_from_slice(&bytes[..len]);
        // Keep C-string termination for BPF-side lookup.
        key[len] = 0;
        let _ = map.remove(&key);
        Ok(())
    }
}
