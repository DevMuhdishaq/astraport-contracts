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
- **Purpose**: Manages staking operations, alerts, and yield calculation
- **Key Functions**:
  - `initialize()` - Initialize the contract
  - `stake()` - Stake assets; opens or grows the staker/asset yield position so its principal tracks the staked balance
  - `unstake()` - Unstake assets; checkpoints accrued yield before reducing principal so no yield is lost
  - `get_balance()` - Query the staked balance for a staker/asset pair
  - `set_yield_defaults()` - Configure the default APR and compounding mode seeded onto newly opened positions
  - `set_alert_threshold()` - Configure alert thresholds
- **Yield Engine Functions**:
  - `open_yield_position()` - Start accruing yield for a staker/asset at a given APR and compounding mode
  - `accrue_yield()` - Checkpoint accrued yield to the current ledger time
  - `current_yield()` - Read real-time yield (checkpointed + pending) without mutation
  - `set_yield_rate()` - Change the APR, time-weighting the prior accrual
  - `yield_history()` - Query the complete, ordered yield history
  - `project_yield()` - Estimate future earnings over a horizon
  - `apr_to_apy()` / `apy_to_apr()` - Convert between nominal and effective rates
  - `schedule_distribution()` / `process_distribution()` - Schedule and process yield payouts
- **Use Cases**:
  - Asset staking and accurate yield generation
  - Daily and continuous compounding models
  - Balance monitoring
  - Alert configurations

##### Yield Engine Design

The yield engine is split into focused, independently testable modules:

- **`fixed_point`** — Deterministic fixed-point math (scale `1e18`) replacing
  floating point: `mul`, `div`, `pow_uint`, `exp` (Taylor series with range
  reduction), and a 256-bit intermediate `mul_div` to avoid overflow. `no_std`,
  no `f64` — every validator computes identical results.
- **`compounding`** — The `CompoundingStrategy` trait with `Daily` and
  `Continuous` variants, plus `YieldCalculator` for yield/balance over a
  duration and across variable-rate segments.
- **`apy`** — `APYCalculator` converting APR ⇄ APY per model, backed by `ln`
  and `nth_root` helpers. Accurate to well within 0.01%.
- **`records`** — Soroban `#[contracttype]` structs: `YieldRecord`,
  `YieldHistoryEntry`, `YieldProjection`, `DistributionSchedule`, keyed by
  `(staker, asset)`.
- **`engine`** — Storage-backed `YieldEngine` performing real-time accrual,
  time-weighted rate changes, append-only history, and distribution scheduling.
- **`projection`** — `YieldProjector` for future-earnings estimates, reusing the
  same compounding math so forecasts match realized accrual.

Financial formulas:
- Daily growth factor: `(1 + r/365)^d`, with linear interest on any partial day.
- Continuous growth factor: `e^(r·t)`.
- APY: `(1 + r/365)^365 − 1` (daily) or `e^r − 1` (continuous).

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
```

## Security Considerations

- All contracts use no_std to minimize attack surface
- Input validation is required for all public functions
- Implement access control mechanisms for sensitive operations
- Regular audits recommended before mainnet deployment
