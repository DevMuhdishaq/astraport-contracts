# AstraPort Smart Contracts - Architecture

## Overview

AstraPort Smart Contracts is a suite of Soroban-based smart contracts built on the Stellar blockchain. The contracts enable decentralized portfolio management with features for rebalancing, event-driven actions, staking, and role-based access control.

## Architecture

### Contract Modules

#### 1. Rebalancing Contract
- **Purpose**: Manages portfolio rebalancing and allocation adjustments with RBAC-secured portfolio-level access control
- **Key Functions**:
  - `initialize()` - Initialize the contract
  - `set_target_allocation(owner, portfolio_id, allocation)` - Set target allocation (owner or MANAGER role required)
  - `set_current_holdings(owner, portfolio_id, holdings)` - Set current holdings (owner or MANAGER role required)
  - `set_schedule(owner, portfolio_id, interval)` - Set rebalancing schedule (owner or MANAGER role required)
  - `rebalance(owner, portfolio_id)` - Execute manual portfolio rebalancing (owner or MANAGER role required)
  - `execute_rebalance(owner, portfolio_id, strategy)` - Execute rebalance with strategy (owner or MANAGER role required)
  - `set_drift_threshold_bps(owner, portfolio_id, threshold)` - Set drift tolerance (owner or MANAGER role required)
  - `get_owner(portfolio_id)` - Query portfolio owner address
  - `get_status(portfolio_id)` - Query rebalancing status
  - `grant_role(granter, portfolio_id, assignee, role, expires_at)` - Assign RBAC role
  - `revoke_role(revoker, portfolio_id, assignee)` - Revoke RBAC role
  - `check_permission_rbac(portfolio_id, actor, permission)` - Check RBAC permissions
  - `get_access_log(portfolio_id)` - Retrieve permission check audit log
- **Use Cases**:
  - Automated portfolio rebalancing
  - Target allocation management
  - Drift correction
  - Delegated portfolio management via RBAC

##### Role-Based Access Control (RBAC)

The rebalancing contract implements a comprehensive RBAC system enabling portfolio owners to delegate specific permissions to other accounts.

**Roles:**

| Role       | Description                                      | Default Permissions                                                                 |
|------------|--------------------------------------------------|-------------------------------------------------------------------------------------|
| Owner      | Full control over portfolio                      | ALL permissions                                                                     |
| Manager    | Can modify allocations and trigger rebalancing   | VIEW + MODIFY_ALLOCATIONS + REBALANCE + MANAGE_SCHEDULE + EXECUTE_REBALANCE         |
| Viewer     | Read-only access to portfolio data               | VIEW                                                                                |
| Liquidator | Emergency withdrawal only                        | VIEW + LIQUIDATE                                                                    |

**Permission Bitmask Constants:**

| Constant               | Bit   | Description                                    |
|------------------------|-------|------------------------------------------------|
| `CAN_VIEW`            | 0x01  | Read portfolio data                            |
| `CAN_MODIFY_ALLOCATIONS` | 0x02 | Modify target allocation, holdings, drift    |
| `CAN_REBALANCE`       | 0x04  | Trigger manual rebalancing                     |
| `CAN_MANAGE_SCHEDULE` | 0x08  | Set, update, or cancel rebalancing schedules   |
| `CAN_EXECUTE_REBALANCE` | 0x10 | Execute rebalance with execution strategy    |
| `CAN_LIQUIDATE`       | 0x20  | Emergency withdrawal                           |
| `CAN_MANAGE_ROLES`    | 0x40  | Assign and revoke roles                        |
| `CAN_CONFIGURE`       | 0x80  | Configure system settings (audit sink, etc.)   |

**Role Inheritance:**
- Manager ⊇ Viewer: Manager includes all Viewer permissions
- Owner inherits all permissions from every other role

**Time-Limited Roles:**
Roles can be assigned with an optional expiry timestamp. Once the ledger timestamp exceeds the expiry, the role is automatically considered revoked. `expires_at = 0` means the role never expires.

**Access Logging:**
Every permission check is recorded in an append-only access log per portfolio, including:
- Actor address
- Required permission
- Actor's actual permissions
- Whether access was granted or denied
- Action name and timestamp

#### 2. Events Contract
- **Purpose**: Emits and manages events on portfolio changes
- **Key Functions**:
  - `initialize()` - Initialize the contract
  - `emit_event()` - Trigger portfolio change events
  - `subscribe()` - Subscribe to portfolio events
  - `unsubscribe()` - Unsubscribe from portfolio events
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

#### 4. Audit Contract
- **Purpose**: Immutable, append-only audit log for portfolio events with tamper detection via SHA-256 chain hash
- **Key Functions**:
  - `initialize(admin)` - Initialize with admin address
  - `log_event(actor, event_type, portfolio, permissions, ...)` - Append audit entry
  - `query(log_query)` - Filter and retrieve audit entries
  - `verify_integrity(expected_head)` - Verify chain hash integrity
  - `prune_old(admin)` - Enforce retention policy

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
- Role-based access control enforces security boundaries on all state-changing functions
- Unauthorized access attempts are logged for audit purposes
- Time-limited roles expire automatically for defense in depth
- Regular audits recommended before mainnet deployment
