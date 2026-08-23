# AstraPort Smart Contracts - Architecture

## Overview

AstraPort Smart Contracts is a suite of Soroban-based smart contracts built on the Stellar blockchain. The contracts enable decentralized portfolio management with features for rebalancing, event-driven actions, and staking.

## Architecture

### Contract Modules

#### 1. Rebalancing Contract
- **Purpose**: Manages portfolio rebalancing and allocation adjustments
- **Key Functions**:
  - `initialize()` - Initialize the contract
  - `rebalance()` - Execute portfolio rebalancing
  - `get_status()` - Query rebalancing status
- **Use Cases**:
  - Automated portfolio rebalancing
  - Target allocation management
  - Drift correction

#### 2. Events Contract
- **Purpose**: Emits and manages events on portfolio changes
- **Key Functions**:
  - `initialize()` - Initialize the contract
  - `emit_event()` - Trigger portfolio change events
  - `subscribe()` - Subscribe to portfolio events
  - `unsubscribe()` - Unsubscribe from events
- **Use Cases**:
  - Portfolio change notifications
  - AI analysis triggers
  - Event-driven automation

#### 3. Staking Contract
- **Purpose**: Manages staking operations and alerts
- **Key Functions**:
  - `initialize()` - Initialize the contract
  - `stake()` - Stake assets
  - `unstake()` - Unstake assets
  - `get_balance()` - Query staking balance
  - `set_alert_threshold()` - Configure alert thresholds
- **Use Cases**:
  - Asset staking and yield generation
  - Balance monitoring
  - Alert configurations

#### 4. Emergency Controls Contract
- **Purpose**: Safety mechanisms and emergency response systems
- **Key Functions**:
  - `pause()` / `unpause()` - Pause/resume contract operations (admin + guardian)
  - `emergency_withdrawal()` - Bypass lock periods with penalty fee
  - `report_price_change()` - Trigger circuit breaker on extreme price moves
  - `reset_circuit_breaker()` - Reset circuit breaker (admin only)
  - `enter_safe_mode()` / `exit_safe_mode()` - Reduce functionality during risk
  - `validate_trade_size()` - Enforce maximum trade size limits
  - `validate_operation()` - Check if operation is allowed in current state
  - `set_rate_limit()` / `check_rate_limit()` - Rate limiting on critical operations
  - `notify()` - Emit notifications to registered watchers
  - `get_incident_log()` - Query incident history
  - `get_emergency_state()` - Comprehensive system state snapshot
- **Use Cases**:
  - Circuit breakers on market crashes (auto-halt at >20% price change)
  - Emergency withdrawals bypassing normal lock periods (with configurable penalty)
  - Pause mechanism preventing new transactions while allowing withdrawals
  - Safe mode disabling automated operations during risk events
  - Rate limiting to prevent abuse of critical functions
  - Full incident audit trail for all emergency actions

## Technology Stack

- **Language**: Rust
- **Framework**: Soroban SDK v21.5.0
- **Blockchain**: Stellar
- **Workspace**: Cargo with multiple contract crates

## Deployment

Contracts are deployed individually to the Stellar blockchain via the Soroban CLI.

```bash
soroban contract build --package astraport-rebalancing
soroban contract build --package astraport-events
soroban contract build --package astraport-staking
soroban contract build --package astraport-emergency
```

## Security Considerations

- All contracts use no_std to minimize attack surface
- Input validation is required for all public functions
- Implement access control mechanisms for sensitive operations
- Regular audits recommended before mainnet deployment
