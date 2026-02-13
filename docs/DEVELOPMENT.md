# Development Guide

## Prerequisites

- Rust 1.70.0 or higher
- Soroban CLI v21.5.0 or higher
- Node.js 18+ (for example helpers, optional)

## Setup

### 1. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

### 2. Install Soroban CLI

```bash
cargo install soroban-cli
```

### 3. Add wasm32 target

```bash
rustup target add wasm32-unknown-unknown
```

### 4. Clone and Setup Repository

```bash
git clone https://github.com/FoxAIhelper/astraport-contracts.git
cd astraport-contracts
cargo build
```

## Building Contracts

### Build all contracts in debug mode

```bash
cargo build
```

### Build for WASM (required for deployment)

```bash
soroban contract build --package astraport-rebalancing
soroban contract build --package astraport-events
soroban contract build --package astraport-staking
```

### Build with optimizations (release mode)

```bash
cargo build --release
```

## Testing

### Run all tests

```bash
cargo test
```

### Run specific contract tests

```bash
cargo test -p astraport-rebalancing
cargo test -p astraport-events
cargo test -p astraport-staking
```

### Run with verbose output

```bash
cargo test -- --nocapture
```

## Project Structure

```
astraport-contracts/
├── Cargo.toml                 # Workspace configuration
├── contracts/
│   ├── rebalancing/           # Rebalancing contract
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   ├── events/                # Events contract
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   └── staking/               # Staking contract
│       ├── Cargo.toml
│       └── src/
│           └── lib.rs
├── docs/                      # Documentation
│   ├── ARCHITECTURE.md
│   └── DEVELOPMENT.md
├── examples/                  # Example implementations
├── tests/                     # Integration tests
└── README.md
```

## Code Style

- Follow Rust naming conventions (snake_case for functions/variables, CamelCase for types)
- Use meaningful variable names
- Add documentation comments to public functions
- Format code with `cargo fmt`
- Check for linting issues with `cargo clippy`

## Deployment

### Testnet Deployment

```bash
# Set your test account (requires Soroban CLI setup)
soroban contract build --package astraport-rebalancing
soroban contract deploy --wasm target/wasm32-unknown-unknown/release/astraport_rebalancing.wasm \
  --source testuser \
  --network testnet
```

### Mainnet Deployment (use with caution)

```bash
soroban contract build --package astraport-rebalancing
soroban contract deploy --wasm target/wasm32-unknown-unknown/release/astraport_rebalancing.wasm \
  --source mainuser \
  --network mainnet
```

## Troubleshooting

### Missing WASM target

```bash
rustup target add wasm32-unknown-unknown
```

### Build errors related to Soroban

Ensure you have the latest Soroban CLI:

```bash
cargo install --force soroban-cli
```

### Testing issues

Clear the build cache if encountering stale test issues:

```bash
cargo clean
cargo test
```
