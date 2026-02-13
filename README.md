# AstraPort Smart Contracts

[![Rust](https://img.shields.io/badge/Rust-1.75+-orange.svg)](https://www.rust-lang.org/)
[![Soroban SDK](https://img.shields.io/badge/Soroban%20SDK-21.5.0-blue.svg)](https://github.com/stellar/rs-soroban-sdk)
[![License](https://img.shields.io/badge/License-Apache%202.0-green.svg)](LICENSE)

AstraPort Smart Contracts are built on **Stellar's Soroban framework**, enabling decentralized portfolio management with automated rebalancing, event-driven actions, and staking capabilities.

---

## 📂 Repository Structure

```
astraport-contracts/
├── contracts/                 # Smart contract crates
│   ├── rebalancing/          # Portfolio rebalancing contract
│   ├── events/               # Event management contract
│   └── staking/              # Asset staking contract
├── docs/                      # Comprehensive documentation
│   ├── ARCHITECTURE.md       # Contract architecture & design
│   └── DEVELOPMENT.md        # Development setup & guidelines
├── examples/                  # Usage examples
├── tests/                     # Integration tests
├── Cargo.toml                # Workspace configuration
├── rust-toolchain.toml       # Rust version specification
└── README.md                 # This file
```

---

## 🔑 Smart Contract Modules

### 1. **Rebalancing Contract**
Manages portfolio rebalancing and allocation adjustments.

**Key Functions:**
- `initialize()` - Initialize the contract
- `rebalance(portfolio_id)` - Execute portfolio rebalancing
- `get_status(portfolio_id)` - Query rebalancing status

**Use Cases:**
- Automated portfolio rebalancing
- Target allocation management
- Drift correction

### 2. **Events Contract**
Emits and manages events on portfolio changes with AI analysis triggers.

**Key Functions:**
- `initialize()` - Initialize the contract
- `emit_event(portfolio_id, change_type)` - Trigger portfolio change events
- `subscribe(portfolio_id, subscriber)` - Subscribe to events
- `unsubscribe(portfolio_id, subscriber)` - Unsubscribe from events

**Use Cases:**
- Portfolio change notifications
- AI analysis triggers
- Event-driven automation

### 3. **Staking Contract**
Manages asset staking with alert thresholds and balance monitoring.

**Key Functions:**
- `initialize()` - Initialize the contract
- `stake(staker, amount)` - Stake assets
- `unstake(staker, amount)` - Unstake assets
- `get_balance(staker)` - Query staking balance
- `set_alert_threshold(threshold)` - Configure alert thresholds

**Use Cases:**
- Asset staking and yield generation
- Balance monitoring
- Threshold-based alerts

---

## 🚀 Quick Start

### Prerequisites

- **Rust** 1.75.0 or higher
- **Soroban CLI** 21.5.0 or higher
- **Node.js** 18+ (optional, for example utilities)

### Installation

1. **Clone the repository**
   ```bash
   git clone https://github.com/FoxAIhelper/astraport-contracts.git
   cd astraport-contracts
   ```

2. **Install Rust** (if not already installed)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source $HOME/.cargo/env
   ```

3. **Install Soroban CLI**
   ```bash
   cargo install soroban-cli
   ```

4. **Add WASM target**
   ```bash
   rustup target add wasm32-unknown-unknown
   ```

5. **Build contracts**
   ```bash
   cargo build
   ```

---

## 🔨 Development

### Build Contracts

**Debug build:**
```bash
cargo build
```

**Release build (optimized for WASM):**
```bash
soroban contract build --package astraport-rebalancing
soroban contract build --package astraport-events
soroban contract build --package astraport-staking
```

### Running Tests

**All tests:**
```bash
cargo test
```

**Specific contract tests:**
```bash
cargo test -p astraport-rebalancing
cargo test -p astraport-events
cargo test -p astraport-staking
```

**With verbose output:**
```bash
cargo test -- --nocapture
```

### Code Quality

**Format code:**
```bash
cargo fmt
```

**Check for linting issues:**
```bash
cargo clippy
```

---

## 📚 Documentation

- [**Architecture Guide**](docs/ARCHITECTURE.md) - Contract design patterns and security considerations
- [**Development Guide**](docs/DEVELOPMENT.md) - Setup, building, testing, and deployment instructions
- [**Usage Examples**](examples/usage_examples.rs) - Example implementations and patterns

---

## 🌐 Deployment

### Testnet Deployment

```bash
soroban contract build --package astraport-rebalancing
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/astraport_rebalancing.wasm \
  --source testuser \
  --network testnet
```

### Mainnet Deployment

```bash
soroban contract build --package astraport-rebalancing
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/astraport_rebalancing.wasm \
  --source mainuser \
  --network mainnet
```

---

## 🛡️ Security

- All contracts use `#![no_std]` to minimize attack surface
- Input validation required for all public functions
- Access control mechanisms recommended for sensitive operations
- Regular security audits recommended before mainnet deployment
- Report security issues to: [security contact to be added]

---

## 🤝 Contributing

Contributions are welcome! Please follow these guidelines:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

---

## 📄 License

This project is licensed under the Apache License 2.0 - see the [LICENSE](LICENSE) file for details.

---

## 📞 Support

For questions and support:
- Open an issue on GitHub
- Check the [docs](docs/) folder for detailed guides
- Review examples in the [examples/](examples/) directory

---

## 🔗 Links

- [Stellar Network](https://stellar.org)
- [Soroban Documentation](https://soroban.stellar.org)
- [Soroban Rust SDK](https://github.com/stellar/rs-soroban-sdk)
- [Soroban CLI](https://github.com/stellar/soroban-cli)
