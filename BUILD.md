# Build Instructions

## Prerequisites

- Windows 10 or later
- Rust toolchain (1.70 or later)
- Visual Studio Build Tools or Visual Studio with C++ workload

## Dependencies

The project uses `native-windows-gui` crate for native Windows UI.

## Build

```bash
cargo build --release
```

This produces `target/release/udpfwd.exe`.

## Running

```bash
cargo run --release
```

Or directly execute:
```bash
target/release/udpfwd.exe
```

## Notes

- Release builds are recommended for production use
- The executable is self-contained
- Requires Windows manifest for COMCTL 6.0 (handled automatically by build.rs)