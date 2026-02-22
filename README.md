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
# list GPUs (table with block indicator)
chainsaw list

# list supported modes
chainsaw list-modes

# check mode
chainsaw get

# set mode
chainsaw set integrated
chainsaw set hybrid

# block/unblock one GPU by numeric id
chainsaw gpu <id> block on
chainsaw gpu <id> block off
```

## Output Notes

- `chainsaw list` prints a GPU table with: `ID`, `NAME`, `PCI`, `RENDER`, `DEFAULT`, `BLOCKED`.
- `BLOCKED=on*` means one or more eBPF block entries are active for that GPU.
- GPU ids are the values shown in the `ID` column and are used by `chainsaw gpu <id> ...`.

## Notes

- The daemon and eBPF logic are experimental.
- Use on test machines only.
