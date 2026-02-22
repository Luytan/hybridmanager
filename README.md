# Chainsaw (Prototype)

Prototype tool to switch dGPU access mode on Linux.

Supported modes:
- `integrated`
- `hybrid`

## Scope

This project is prototype-only.
Expect breakage, hardware-specific behavior, and missing safety checks.

## Requirements

- Linux with IOMMU enabled
- `/sys/bus/pci` and `/dev/dri` available
- D-Bus system bus
- Rust toolchain or Nix
- Root privileges for daemon/eBPF operations

## Quick Start

```bash
# build
nix develop -c cargo build

# or
make build
```

## Prototype Flow

```bash
# list modes
chainsaw list

# check mode
chainsaw get

# set mode
chainsaw set integrated
chainsaw set hybrid
```

## Notes

- The daemon and eBPF logic are experimental.
- Use on test machines only.
