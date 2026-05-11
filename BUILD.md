# Build Instructions

## Prerequisites

- Windows 10 or later
- Rust toolchain (1.85+; edition 2024)
- Visual Studio Build Tools (or Visual Studio with the C++ workload)

## Build

```bash
cargo build --release
```

## Run

```bash
cargo run --release
```

## Test

```bash
cargo test
```

For loopback and performance tests, build the release binary first, then:

```bash
python test/test_udp_loopback.py
```
