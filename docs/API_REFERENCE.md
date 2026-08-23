# AstraPort Smart Contracts — API Reference

> Generated from the `main` branch of `astraport-contracts`.
> All amounts are in **base units** (1 XLM = 10,000,000 stroops).
> Fixed-point rates use **SCALE = 1e18** (e.g., 5% APR → `50_000_000_000_000_000`).

---

## Table of Contents

- [1. Staking Contract](#1-staking-contract)
  - [1.1 Lifecycle](#11-lifecycle)
  - [1.2 Staking](#12-staking)
  - [1.3 Protocol Totals](#13-protocol-totals)
  - [1.4 Lock Positions](#14-lock-positions)
  - [1.5 Emergency Unstaking](#15-emergency-unstaking)
  - [1.6 Yield Engine](#16-yield-engine)
  - [1.7 Distribution Scheduling](#17-distribution-scheduling)
  - [1.8 Admin](#18-admin)
  - [1.9 Audit Integration](#19-audit-integration)
  - [1.10 Errors](#110-errors)
  - [1.11 Types](#111-types)
- [2. Rebalancing Contract](#2-rebalancing-contract)
  - [2.1 Lifecycle](#21-lifecycle)
  - [2.2 Ownership & Access Control](#22-ownership--access-control)
  - [2.3 Target Allocation](#23-target-allocation)
  - [2.4 Current Holdings](#24-current-holdings)
  - [2.5 Drift Threshold](#25-drift-threshold)
  - [2.6 Rebalancing](#26-rebalancing)
  - [2.7 Scheduling](#27-scheduling)
  - [2.8 Multi-Asset Execution](#28-multi-asset-execution)
  - [2.9 Audit Integration](#29-audit-integration)
  - [2.10 Errors](#210-errors)
  - [2.11 Types](#211-types)
- [3. Events Contract](#3-events-contract)
  - [3.1 Lifecycle](#31-lifecycle)
  - [3.2 AI Triggers](#32-ai-triggers)
  - [3.3 Event Processing](#33-event-processing)
  - [3.4 Analysis Management](#34-analysis-management)
  - [3.5 Recommendations](#35-recommendations)
  - [3.6 Subscriptions](#36-subscriptions)
  - [3.7 Queries](#37-queries)
  - [3.8 Errors](#38-errors)
  - [3.9 Types](#39-types)
- [4. Audit Contract](#4-audit-contract)
  - [4.1 Lifecycle](#41-lifecycle)
  - [4.2 Retention Policy](#42-retention-policy)
  - [4.3 Logging](#43-logging)
  - [4.4 Querying](#44-querying)
  - [4.5 Integrity Verification](#45-integrity-verification)
  - [4.6 Pruning](#46-pruning)
  - [4.7 Export](#47-export)
  - [4.8 Errors](#48-errors)
  - [4.9 Types](#49-types)

---

## 1. Staking Contract

**Package:** `astraport-staking`
**Entry:** `StakingContract`

Manages multi-asset staking, yield accrual, emergency early withdrawal with time-decaying penalties, alert thresholds, and protocol-level totals.

### 1.1 Lifecycle

#### `initialize(env, admin) → Symbol`

Initialize the staking contract with an admin address.

| Parameter | Type | Description |
|-----------|------|-------------|
| `env` | `Env` | Soroban environment |
| `admin` | `Address` | Admin address for the contract |

**Returns:** `ok` (Symbol)
**Panics:** If called more than once (`"already initialized"`)
**Auth:** None

---

### 1.2 Staking

#### `stake(env, staker, asset, amount) → Result<Symbol, Error>`

Stake `amount` of `asset` into the contract.

| Parameter | Type | Description |
|-----------|------|-------------|
| `env` | `Env` | Soroban environment |
| `staker` | `Address` | Staker's address (must authorize) |
| `asset` | `Symbol` | Asset symbol (e.g., `XLM`, `USDC`) |
| `amount` | `i128` | Amount to stake (must be > 0) |

**Returns:** `Ok(ok)` on success.
**Errors:** `InvalidStakeAmount` if `amount <= 0` or addition overflows.
**Side effects:** Updates `TotalStaked(asset)`, `ActiveStakerCount`, emits `StakeEvent`, logs audit event.
**Auth:** `staker.require_auth()`

---

#### `unstake(env, staker, asset, amount) → Result<Symbol, Error>`

Unstake `amount` of `asset` from the contract (normal, after lock expiry).

| Parameter | Type | Description |
|-----------|------|-------------|
| `env` | `Env` | Soroban environment |
| `staker` | `Address` | Staker's address (must authorize) |
| `asset` | `Symbol` | Asset symbol |
| `amount` | `i128` | Amount to unstake (must be > 0) |

**Returns:** `Ok(ok)` on success.
**Errors:** `InvalidStakeAmount` if `amount <= 0`; `InsufficientBalance` if `amount > balance`.
**Side effects:** Updates `TotalStaked(asset)`, `ActiveStakerCount`, emits `UnstakeEvent`, logs audit event. Removes balance key if balance reaches 0.
**Auth:** `staker.require_auth()`

---

#### `get_balance(env, staker, asset) → i128`

Return the staked balance for a `(staker, asset)` pair.

| Parameter | Type | Description |
|-----------|------|-------------|
| `env` | `Env` | Soroban environment |
| `staker` | `Address` | Staker's address |
| `asset` | `Symbol` | Asset symbol |

**Returns:** Balance in base units, or `0` if no position exists.
**Auth:** None (read-only)

---

### 1.3 Protocol Totals

#### `total_staked(env, asset) → i128`

Total amount of `asset` currently staked across every staker.

| Parameter | Type | Description |
|-----------|------|-------------|
| `env` | `Env` | Soroban environment |
| `asset` | `Symbol` | Asset symbol |

**Returns:** Aggregate staked amount in base units.
**Auth:** None (read-only)

---

#### `staker_count(env) → u32`

Number of distinct stakers with at least one non-zero balance across any asset.

| Parameter | Type | Description |
|-----------|------|-------------|
| `env` | `Env` | Soroban environment |

**Returns:** Count of active stakers.
**Auth:** None (read-only)

---

### 1.4 Lock Positions

#### `set_lock_position(env, admin, staker, lock_start_ts, unlock_ts, locked_amount) → Symbol`

Record a lock-up period for a staker (admin-only).

| Parameter | Type | Description |
|-----------|------|-------------|
| `env` | `Env` | Soroban environment |
| `admin` | `Address` | Admin address (must authorize + be admin) |
| `staker` | `Address` | Staker's address |
| `lock_start_ts` | `u64` | Ledger timestamp when lock started |
| `unlock_ts` | `u64` | Ledger timestamp when lock expires |
| `locked_amount` | `i128` | Total locked principal |

**Returns:** `ok`
**Auth:** `admin.require_auth()` + admin check

---

#### `get_lock_position(env, staker) → Option<LockPosition>`

Query the lock position for a staker, if any.

| Parameter | Type | Description |
|-----------|------|-------------|
| `env` | `Env` | Soroban environment |
| `staker` | `Address` | Staker's address |

**Returns:** `Some(LockPosition)` if a lock exists, `None` otherwise.
**Auth:** None (read-only)

---

### 1.5 Emergency Unstaking

#### `configure_emergency_unstake(env, admin, penalty_start_bps, penalty_end_bps, decay_function, cooldown_seconds, treasury, enabled) → Symbol`

Configure the emergency-unstake system (admin-only). Calling a second time overwrites.

| Parameter | Type | Description |
|-----------|------|-------------|
| `env` | `Env` | Soroban environment |
| `admin` | `Address` | Admin address |
| `penalty_start_bps` | `i128` | Penalty at lock start (0–10,000 bps) |
| `penalty_end_bps` | `i128` | Penalty at unlock date (0–10,000 bps) |
| `decay_function` | `PenaltyDecayFunction` | `Linear`, `Exponential`, or `Custom` |
| `cooldown_seconds` | `u64` | Wait between emergency unstakes (0 disables) |
| `treasury` | `Address` | Address receiving collected penalties |
| `enabled` | `bool` | Whether emergency unstaking is active |

**Returns:** `ok`
**Auth:** `admin.require_auth()` + admin check

---

#### `emergency_unstake(env, staker, asset, amount) → EmergencyUnstakeRecord`

Perform an emergency unstake before the lock-up period expires.

| Parameter | Type | Description |
|-----------|------|-------------|
| `env` | `Env` | Soroban environment |
| `staker` | `Address` | Staker's address (must authorize) |
| `asset` | `Symbol` | Asset symbol |
| `amount` | `i128` | Amount to emergency-unstake |

**Returns:** `EmergencyUnstakeRecord` with penalty details.
**Panics:** If disabled, cooldown active, insufficient balance, or invalid amount.
**Side effects:** Deducts penalty, distributes to treasury via event, activates cooldown, appends to history.
**Auth:** `staker.require_auth()`

---

#### `get_emergency_config(env) → Option<EmergencyUnstakeConfig>`

Return the emergency-unstake configuration, if initialized.

**Auth:** None (read-only)

---

#### `get_cooldown_end(env, staker) → u64`

Return the timestamp after which `staker` may emergency-unstake again. Returns `0` if no cooldown.

**Auth:** None (read-only)

---

#### `is_in_cooldown(env, staker) → bool`

Return `true` if `staker` is currently in a cooldown period.

**Auth:** None (read-only)

---

#### `get_emergency_unstake_history(env, staker) → Vec<EmergencyUnstakeRecord>`

Full emergency-unstake history for `staker`, oldest first.

**Auth:** None (read-only)

---

#### `preview_emergency_penalty(env, lock_start_ts, unlock_ts) → Option<i128>`

Preview the penalty basis points for a hypothetical emergency unstake without mutating storage.

**Auth:** None (read-only)

---

### 1.6 Yield Engine

#### `open_yield_position(env, staker, asset, principal, apr, mode) → YieldRecord`

Open or reset a yield-accruing position for a staker and asset.

| Parameter | Type | Description |
|-----------|------|-------------|
| `staker` | `Address` | Staker's address |
| `asset` | `Symbol` | Asset symbol |
| `principal` | `i128` | Principal in base units |
| `apr` | `i128` | Annual percentage rate (fixed-point) |
| `mode` | `CompoundingMode` | `Daily` or `Continuous` |

**Returns:** `YieldRecord` with position details.
**Auth:** None (typically called internally)

---

#### `accrue_yield(env, staker, asset) → YieldRecord`

Checkpoint a position, realizing all yield accrued up to the current ledger time.

**Returns:** Updated `YieldRecord`.
**Auth:** None (typically called internally)

---

#### `claim_yield(env, staker, asset) → i128`

Claim all yield accrued by a staker for an asset.

**Returns:** Amount claimed in base units.
**Side effects:** Resets `accrued_yield` to 0, appends claim marker to history.
**Auth:** `staker.require_auth()`

---

#### `current_yield(env, staker, asset) → i128`

The total yield a position has earned as of now, without mutating storage.

**Returns:** Yield in base units (checkpointed + pending).
**Auth:** None (read-only)

---

#### `set_yield_rate(env, staker, asset, new_apr) → YieldRecord`

Change the APR for a position, checkpointing prior yield at the old rate.

**Returns:** Updated `YieldRecord`.
**Auth:** None (typically called internally)

---

#### `yield_history(env, staker, asset) → Vec<YieldHistoryEntry>`

The complete yield history for a staker/asset pair, oldest entry first.

**Auth:** None (read-only)

---

#### `project_yield(env, principal, apr, mode, horizon_seconds) → YieldProjection`

Project future earnings over a horizon.

| Parameter | Type | Description |
|-----------|------|-------------|
| `principal` | `i128` | Principal in base units |
| `apr` | `i128` | APR (fixed-point) |
| `mode` | `CompoundingMode` | Compounding model |
| `horizon_seconds` | `u64` | Projection horizon in seconds |

**Returns:** `YieldProjection` with projected yield, balance, and effective APY.
**Auth:** None (pure computation)

---

#### `apr_to_apy(env, apr, mode) → i128`

Convert a nominal APR to its effective APY.

**Auth:** None (pure computation)

---

#### `apy_to_apr(env, apy, mode) → i128`

Convert an effective APY back to its nominal APR.

**Auth:** None (pure computation)

---

### 1.7 Distribution Scheduling

#### `schedule_distribution(env, staker, asset, amount, due_ts, interval_seconds) → DistributionSchedule`

Schedule a yield distribution to a staker. `interval_seconds = 0` makes it a one-off.

**Returns:** `DistributionSchedule` record.
**Auth:** None (typically called internally)

---

#### `process_distribution(env, staker, asset) → i128`

Process due distributions for a staker/asset pair. Returns total amount marked due.

**Returns:** Total amount distributed in base units.
**Auth:** None (typically called internally)

---

### 1.8 Admin

#### `set_alert_threshold(env, admin, threshold) → Symbol`

Set the alert threshold for staking changes. Admin-only.

**Auth:** `admin.require_auth()` + admin check

---

#### `set_yield_defaults(env, default_apr, default_mode)`

Reconfigure the default APR and compounding mode for new yield positions.

| Parameter | Type | Description |
|-----------|------|-------------|
| `default_apr` | `i128` | Default APR (fixed-point) |
| `default_mode` | `CompoundingMode` | Default compounding model |

**Auth:** None (public but typically admin-gated off-chain)

---

### 1.9 Audit Integration

#### `set_audit_sink(env, admin, sink) → Symbol`

Configure the audit-log contract address. Admin-only.

**Auth:** `admin.require_auth()` + admin check

---

#### `get_audit_sink(env) → Option<Address>`

Read the audit-log sink address, if configured.

**Auth:** None (read-only)

---

### 1.10 Errors

| Error | Value | Description |
|-------|-------|-------------|
| `InvalidStakeAmount` | 1 | Amount must be positive |
| `InsufficientBalance` | 2 | Cannot unstake more than staked |
| `EmergencyUnstakeDisabled` | 3 | Emergency unstaking is off |
| `CooldownActive` | 4 | Must wait before next emergency unstake |
| `EmergencyConfigNotInitialized` | 5 | Config not set |
| `InvalidEmergencyUnstakeAmount` | 6 | Amount ≤ 0 |

---

### 1.11 Types

#### `CompoundingMode`

| Variant | Description |
|---------|-------------|
| `Daily` | Compounded once per day (365 periods/year) |
| `Continuous` | Compounded continuously (`e^(rt)`) |

#### `YieldRecord`

| Field | Type | Description |
|-------|------|-------------|
| `staker` | `Address` | Position owner |
| `asset` | `Symbol` | Asset being staked |
| `principal` | `i128` | Currently staked principal |
| `apr` | `i128` | Current APR (fixed-point) |
| `mode` | `CompoundingMode` | Compounding model |
| `last_accrual_ts` | `u64` | Last checkpoint timestamp |
| `accrued_yield` | `i128` | Checkpointed yield |

#### `YieldHistoryEntry`

| Field | Type | Description |
|-------|------|-------------|
| `timestamp` | `u64` | Entry timestamp |
| `period_seconds` | `u64` | Duration of this entry |
| `apr` | `i128` | APR during this period |
| `yield_earned` | `i128` | Yield earned in this period |
| `cumulative_yield` | `i128` | Cumulative unclaimed yield |
| `is_claim` | `bool` | `true` for claim markers |

#### `YieldProjection`

| Field | Type | Description |
|-------|------|-------------|
| `principal` | `i128` | Base principal |
| `apr` | `i128` | Assumed APR |
| `mode` | `CompoundingMode` | Compounding model |
| `horizon_seconds` | `u64` | Projection horizon |
| `projected_yield` | `i128` | Expected yield |
| `projected_balance` | `i128` | Expected total balance |
| `effective_apy` | `i128` | Implied effective APY |

#### `DistributionSchedule`

| Field | Type | Description |
|-------|------|-------------|
| `staker` | `Address` | Recipient |
| `asset` | `Symbol` | Asset to distribute |
| `due_ts` | `u64` | Next due timestamp |
| `interval_seconds` | `u64` | Recurring interval (0 = one-off) |
| `amount` | `i128` | Distribution amount |
| `executed` | `bool` | Whether one-off is done |

#### `LockPosition`

| Field | Type | Description |
|-------|------|-------------|
| `staker` | `Address` | Staker's address |
| `lock_start_ts` | `u64` | Lock start timestamp |
| `unlock_ts` | `u64` | Lock expiry timestamp |
| `locked_amount` | `i128` | Locked principal |

#### `EmergencyUnstakeConfig`

| Field | Type | Description |
|-------|------|-------------|
| `penalty_start_bps` | `i128` | Penalty at lock start (bps) |
| `penalty_end_bps` | `i128` | Penalty at unlock (bps) |
| `decay_function` | `PenaltyDecayFunction` | Decay model |
| `cooldown_seconds` | `u64` | Cooldown between emergency unstakes |
| `treasury` | `Address` | Penalty recipient |
| `enabled` | `bool` | Whether active |

#### `EmergencyUnstakeRecord`

| Field | Type | Description |
|-------|------|-------------|
| `staker` | `Address` | Staker |
| `timestamp` | `u64` | Operation timestamp |
| `amount_requested` | `i128` | Requested amount |
| `penalty_amount` | `i128` | Penalty deducted |
| `amount_returned` | `i128` | Amount after penalty |
| `penalty_bps_applied` | `i128` | Actual penalty rate |
| `original_unlock_ts` | `u64` | Original unlock time |
| `lock_start_ts` | `u64` | Lock start time |
| `is_partial` | `bool` | Partial vs full withdrawal |

#### `PenaltyDecayFunction`

| Variant | Description |
|---------|-------------|
| `Linear` | Uniform decay from start to end |
| `Exponential` | Fast early, slow near end |
| `Custom` | Fixed at `penalty_start_bps` |

#### `StakeEvent`

| Field | Type | Description |
|-------|------|-------------|
| `staker` | `Address` | Staker |
| `asset` | `Symbol` | Asset |
| `amount` | `i128` | Amount staked |
| `new_balance` | `i128` | Balance after stake |

#### `UnstakeEvent`

| Field | Type | Description |
|-------|------|-------------|
| `staker` | `Address` | Staker |
| `asset` | `Symbol` | Asset |
| `amount` | `i128` | Amount unstaked |
| `new_balance` | `i128` | Balance after unstake |

#### `StakingConfig`

| Field | Type | Description |
|-------|------|-------------|
| `default_apr` | `i128` | Default APR for new positions |
| `default_mode` | `CompoundingMode` | Default compounding model |

---

## 2. Rebalancing Contract

**Package:** `astraport-rebalancing`
**Entry:** `RebalancingContract`

Manages portfolio rebalancing with owner-gated access control, drift detection, scheduling, and multi-asset execution.

### 2.1 Lifecycle

#### `initialize(env) → Symbol`

Initialize the rebalancing contract.

**Returns:** `ok`
**Auth:** None

---

### 2.2 Ownership & Access Control

#### `get_owner(env, portfolio_id) → Option<Address>`

Get the owner address for a portfolio.

**Auth:** None (read-only)

---

All mutating portfolio functions enforce ownership via `require_owner_auth`. The first caller to modify a portfolio becomes its owner. Subsequent calls require that same address.

---

### 2.3 Target Allocation

#### `set_target_allocation(env, owner, portfolio_id, allocation) → Result<Symbol, RebalancingError>`

Set the target allocation for a portfolio.

| Parameter | Type | Description |
|-----------|------|-------------|
| `owner` | `Address` | Portfolio owner |
| `portfolio_id` | `Symbol` | Portfolio identifier |
| `allocation` | `TargetAllocation` | Asset → weight (bps) map |

**Returns:** `Ok(ok)` if valid.
**Errors:** `InvalidAllocation` if weights don't sum to 10,000; `Unauthorized` if caller is not owner.
**Auth:** `owner.require_auth()`

---

#### `get_target_allocation(env, portfolio_id) → Option<TargetAllocation>`

Get the target allocation for a portfolio.

**Auth:** None (read-only)

---

### 2.4 Current Holdings

#### `set_current_holdings(env, owner, portfolio_id, holdings) → Result<Symbol, RebalancingError>`

Store current portfolio weights (must total 10,000 bps).

**Errors:** `InvalidCurrentHoldings` if weights don't sum to 10,000; `Unauthorized` if not owner.
**Auth:** `owner.require_auth()`

---

#### `get_current_holdings(env, portfolio_id) → Option<CurrentHoldings>`

Get current holdings for a portfolio.

**Auth:** None (read-only)

---

### 2.5 Drift Threshold

#### `set_drift_threshold_bps(env, owner, portfolio_id, threshold_bps) → Result<(), RebalancingError>`

Set the per-portfolio drift tolerance in basis points. Default is 100 bps.

**Auth:** `owner.require_auth()`

---

#### `get_drift_threshold_bps(env, portfolio_id) → u32`

Get the drift threshold. Returns 100 if not configured.

**Auth:** None (read-only)

---

### 2.6 Rebalancing

#### `rebalance(env, owner, portfolio_id) → Result<RebalanceResult, RebalancingError>`

Compute and record a rebalance plan. Only flags assets with drift exceeding the threshold.

**Returns:** `RebalanceResult` with adjustments and threshold.
**Auth:** `owner.require_auth()`

---

#### `get_rebalance_plan(env, portfolio_id) → Result<RebalanceResult, RebalancingError>`

Compute a rebalance plan without recording execution history.

**Auth:** None (read-only)

---

#### `get_status(env, portfolio_id) → Symbol`

Get current rebalancing status.

**Returns:** `ok`
**Auth:** None (read-only)

---

### 2.7 Scheduling

#### `set_schedule(env, owner, portfolio_id, interval) → Symbol`

Set a rebalancing schedule. Cannot overwrite an existing schedule.

| Parameter | Type | Description |
|-----------|------|-------------|
| `interval` | `RebalanceInterval` | `Hourly`, `Daily`, `Weekly`, or `Monthly` |

**Returns:** `ok`, `err_auth`, `err_val`, or `err_exist`.
**Auth:** `owner.require_auth()`

---

#### `update_schedule(env, owner, portfolio_id, interval) → Symbol`

Update an existing rebalancing schedule's interval.

**Returns:** `ok`, `err_auth`, `err_val`, or `err_none`.
**Auth:** `owner.require_auth()`

---

#### `cancel_schedule(env, owner, portfolio_id) → Symbol`

Cancel and remove a rebalancing schedule.

**Returns:** `ok`, `err_auth`, or `err_none`.
**Auth:** `owner.require_auth()`

---

#### `get_schedule(env, portfolio_id) → Option<RebalancingSchedule>`

Get the current schedule for a portfolio.

**Auth:** None (read-only)

---

#### `check_exec_sched_rebalance(env, portfolio_id) → Symbol`

Check if a scheduled rebalance is due and execute it.

**Returns:** `done`, `not_due`, `no_target`, `no_hold`, `err`, or `err_none`.
**Auth:** None (typically called by keeper)

---

#### `get_execution_history(env, portfolio_id) → Vec<ExecutionHistoryRecord>`

Get execution history for a portfolio.

**Auth:** None (read-only)

---

### 2.8 Multi-Asset Execution

#### `execute_rebalance(env, owner, portfolio_id, strategy) → Result<(), RebalancingError>`

Execute a rebalance using the multi-asset rebalancer with a given strategy.

| Parameter | Type | Description |
|-----------|------|-------------|
| `strategy` | `ExecutionStrategy` | `MinimalCost`, `MinimalTime`, or `Balanced` |

**Auth:** `owner.require_auth()`

---

#### `simulate_rebalance(env, portfolio_id, strategy) → Result<SimulationResult, RebalancingError>`

Simulate a rebalance without executing trades. Returns trades, fees, and slippage.

**Auth:** None (read-only)

---

### 2.9 Audit Integration

#### `set_audit_sink(env, sink) → Symbol`

Configure the audit-log contract address.

**Auth:** None (typically deployer-gated)

---

#### `get_audit_sink(env) → Option<Address>`

Read the audit-log sink address.

**Auth:** None (read-only)

---

### 2.10 Errors

| Error | Value | Description |
|-------|-------|-------------|
| `InvalidAllocation` | 1 | Weights don't sum to 10,000 bps |
| `InvalidCurrentHoldings` | 2 | Weights don't sum to 10,000 bps |
| `TargetAllocationNotFound` | 3 | No target allocation set |
| `CurrentHoldingsNotFound` | 4 | No current holdings set |
| `MultiAssetRebalanceFailed` | 5 | Multi-asset execution error |
| `Unauthorized` | 6 | Caller is not portfolio owner |

---

### 2.11 Types

#### `RebalanceInterval`

| Variant | Seconds |
|---------|---------|
| `Hourly` | 3,600 |
| `Daily` | 86,400 |
| `Weekly` | 604,800 |
| `Monthly` | 2,592,000 |

#### `TargetAllocation`

| Field | Type | Description |
|-------|------|-------------|
| `allocations` | `Map<Symbol, u32>` | Asset → weight in bps |

#### `CurrentHoldings`

| Field | Type | Description |
|-------|------|-------------|
| `allocations` | `Map<Symbol, u32>` | Asset → weight in bps |

#### `RebalanceResult`

| Field | Type | Description |
|-------|------|-------------|
| `portfolio_id` | `Symbol` | Portfolio identifier |
| `drift_threshold_bps` | `u32` | Threshold used |
| `adjustments` | `Vec<RebalanceAdjustment>` | Required adjustments |

#### `RebalanceAdjustment`

| Field | Type | Description |
|-------|------|-------------|
| `asset` | `Symbol` | Asset to adjust |
| `current_weight_bps` | `u32` | Current weight |
| `target_weight_bps` | `u32` | Target weight |
| `drift_bps` | `i32` | Signed drift |
| `direction` | `RebalanceDirection` | `Buy` or `Sell` |

#### `RebalanceDirection`

| Variant | Description |
|---------|-------------|
| `Buy` | Underweight — buy more |
| `Sell` | Overweight — sell excess |

#### `RebalancingSchedule`

| Field | Type | Description |
|-------|------|-------------|
| `portfolio_id` | `Symbol` | Portfolio identifier |
| `interval` | `RebalanceInterval` | Schedule interval |
| `next_execution` | `u64` | Next execution timestamp |
| `last_execution` | `u64` | Last execution timestamp (0 = never) |

#### `ExecutionHistoryRecord`

| Field | Type | Description |
|-------|------|-------------|
| `timestamp` | `u64` | Execution timestamp |
| `outcome` | `Symbol` | Result status |
| `details` | `Symbol` | Execution context |

#### `ExecutionStrategy`

| Variant | Description |
|---------|-------------|
| `MinimalCost` | Optimize for lowest fees |
| `MinimalTime` | Optimize for speed |
| `Balanced` | Balance cost and speed |

#### `SimulationResult`

| Field | Type | Description |
|-------|------|-------------|
| `trades` | `Vec<Trade>` | Planned trades |
| `total_fee` | `u128` | Total estimated fees |
| `slippage_bps` | `u128` | Estimated slippage (bps) |

#### `Trade`

| Field | Type | Description |
|-------|------|-------------|
| `asset_to_sell` | `Symbol` | Source asset |
| `asset_to_buy` | `Symbol` | Destination asset |
| `amount_to_sell` | `u128` | Sell amount |
| `expected_amount_to_buy` | `u128` | Expected buy amount |

---

## 3. Events Contract

**Package:** `astraport-events`
**Entry:** `EventsContract`

AI analysis trigger framework for portfolio events. Manages triggers, processes events, and routes analysis results to recommendations.

### 3.1 Lifecycle

#### `initialize(env) → Symbol`

Initialize the events contract and metrics.

**Returns:** `OK`
**Auth:** None

---

### 3.2 AI Triggers

#### `add_trigger(env, trigger) → Result<Symbol, Error>`

Add a new AI trigger to the system.

| Parameter | Type | Description |
|-----------|------|-------------|
| `trigger` | `AITrigger` | Trigger configuration |

**Returns:** `Ok(OK)` on success.
**Errors:** `AlreadyExists` if trigger_id is taken.
**Auth:** `trigger.owner.require_auth()`

---

#### `remove_trigger(env, trigger_id, owner) → Result<Symbol, Error>`

Remove an existing AI trigger.

**Errors:** `NotFound` or `Unauthorized` if caller is not trigger owner.
**Auth:** `owner.require_auth()`

---

### 3.3 Event Processing

#### `process_event(env, portfolio_id, event_type, event_data, current_value) → Result<Vec<u64>, Error>`

Process an event, evaluate triggers, and invoke AI analysis if conditions are met.

| Parameter | Type | Description |
|-----------|------|-------------|
| `portfolio_id` | `Symbol` | Portfolio identifier |
| `event_type` | `u32` | `EventType` as u32 |
| `event_data` | `Bytes` | Additional event data |
| `current_value` | `Option<U256>` | Optional value for threshold evaluation |

**Returns:** Vector of analysis IDs triggered.
**Auth:** None

---

### 3.4 Analysis Management

#### `update_analysis_status(env, analysis_id, status, latency_ms, raw_output, error) → Result<Symbol, Error>`

Update the status of an analysis. On `Completed`, generates a recommendation.

| Parameter | Type | Description |
|-----------|------|-------------|
| `analysis_id` | `u64` | Analysis identifier |
| `status` | `u32` | `AnalysisStatus` as u32 |
| `latency_ms` | `Option<u64>` | Latency in milliseconds |
| `raw_output` | `Option<Bytes>` | AI output bytes |
| `error` | `Option<Symbol>` | Error message if failed |

**Auth:** None

---

#### `process_timeout(env, analysis_id) → Result<Symbol, Error>`

Mark a pending/in-progress analysis as timed out.

**Auth:** None

---

### 3.5 Recommendations

#### `process_recommendation_feedback(env, recommendation_id, accepted, responder) → Result<Symbol, Error>`

Accept or reject a recommendation.

**Auth:** `responder.require_auth()`

---

### 3.6 Subscriptions

#### `subscribe(env, portfolio_id, subscriber) → Result<Symbol, Error>`

Subscribe to portfolio events.

**Auth:** `subscriber.require_auth()`

---

#### `unsubscribe(env, portfolio_id, subscriber) → Result<Symbol, Error>`

Unsubscribe from portfolio events.

**Auth:** `subscriber.require_auth()`

---

### 3.7 Queries

#### `get_portfolio_analyses(env, portfolio_id) → Vec<AnalysisResult>`

Get all analysis results for a portfolio.

**Auth:** None (read-only)

---

#### `get_portfolio_recommendations(env, portfolio_id) → Vec<Recommendation>`

Get all recommendations for a portfolio.

**Auth:** None (read-only)

---

#### `get_metrics(env) → AnalysisMetrics`

Get current analysis metrics.

**Auth:** None (read-only)

---

#### `get_all_triggers(env) → Vec<AITrigger>`

Get all registered triggers.

**Auth:** None (read-only)

---

### 3.8 Errors

| Error | Value | Description |
|-------|-------|-------------|
| `AlreadyExists` | 1 | Trigger ID already taken |
| `NotFound` | 2 | Trigger or analysis not found |
| `Unauthorized` | 3 | Caller is not trigger owner |
| `InvalidState` | 4 | Cannot timeout a completed analysis |

---

### 3.9 Types

#### `AITrigger`

| Field | Type | Description |
|-------|------|-------------|
| `trigger_id` | `Symbol` | Unique identifier |
| `name` | `Symbol` | Human-readable name |
| `event_types` | `Vec<u32>` | Event types that activate this trigger |
| `threshold` | `Option<U256>` | Optional threshold for evaluation |
| `operator` | `Option<u32>` | Comparison operator |
| `ai_service_endpoint` | `Address` | AI service address |
| `timeout` | `u64` | Timeout in milliseconds |
| `is_active` | `bool` | Whether trigger is active |
| `owner` | `Address` | Trigger owner |

#### `EventType` (u32)

| Variant | Value | Description |
|---------|-------|-------------|
| `PortfolioRebalance` | 0 | Portfolio was rebalanced |
| `TradeExecuted` | 1 | Trade was executed |
| `PriceThresholdCrossed` | 2 | Price crossed threshold |
| `VolatilitySpike` | 3 | Volatility spike detected |
| `LiquidityChange` | 4 | Liquidity changed |
| `CustomEvent` | 99 | Custom event |

#### `ComparisonOperator` (u32)

| Variant | Value | Description |
|---------|-------|-------------|
| `GreaterThan` | 0 | `>` |
| `LessThan` | 1 | `<` |
| `EqualTo` | 2 | `==` |
| `GreaterOrEqual` | 3 | `>=` |
| `LessOrEqual` | 4 | `<=` |

#### `AnalysisStatus` (u32)

| Variant | Value | Description |
|---------|-------|-------------|
| `Pending` | 0 | Awaiting processing |
| `InProgress` | 1 | Being processed |
| `Completed` | 2 | Successfully completed |
| `Failed` | 3 | Processing failed |
| `TimedOut` | 4 | Exceeded timeout |

#### `RecommendationType` (u32)

| Variant | Value | Description |
|---------|-------|-------------|
| `Hold` | 0 | Hold position |
| `Buy` | 1 | Buy recommendation |
| `Sell` | 2 | Sell recommendation |
| `Rebalance` | 3 | Rebalance recommendation |
| `Monitor` | 4 | Continue monitoring |
| `NoAction` | 5 | No action needed |

#### `AnalysisResult`

| Field | Type | Description |
|-------|------|-------------|
| `analysis_id` | `u64` | Unique identifier |
| `trigger_id` | `Symbol` | Trigger that fired |
| `portfolio_id` | `Symbol` | Portfolio analyzed |
| `timestamp` | `u64` | Creation timestamp |
| `latency_ms` | `u64` | Processing latency |
| `status` | `u32` | Current status |
| `raw_output` | `Bytes` | Raw AI output |
| `error_message` | `Option<Symbol>` | Error if failed |

#### `Recommendation`

| Field | Type | Description |
|-------|------|-------------|
| `recommendation_id` | `u64` | Unique identifier |
| `analysis_id` | `u64` | Source analysis |
| `portfolio_id` | `Symbol` | Portfolio |
| `action_type` | `u32` | Recommended action |
| `asset` | `Option<Symbol>` | Specific asset (if applicable) |
| `amount` | `Option<U256>` | Suggested amount |
| `confidence_score` | `u32` | Confidence (0–100) |
| `timestamp` | `u64` | Creation timestamp |
| `accepted` | `Option<bool>` | User feedback |

#### `AnalysisMetrics`

| Field | Type | Description |
|-------|------|-------------|
| `total_analyses` | `u64` | Total analyses submitted |
| `successful_analyses` | `u64` | Completed successfully |
| `failed_analyses` | `u64` | Failed |
| `timed_out_analyses` | `u64` | Timed out |
| `average_latency_ms` | `u64` | Running average latency |
| `recommendations_accepted` | `u64` | Accepted recommendations |
| `recommendations_rejected` | `u64` | Rejected recommendations |

---

## 4. Audit Contract

**Package:** `astraport-audit`
**Entry:** `AuditContract`

Immutable, append-only audit log with SHA-256 chain-hash integrity, flexible querying, retention policies, and JSON/CSV export.

### 4.1 Lifecycle

#### `initialize(env, admin) → Symbol`

Initialize the audit contract with an admin address.

**Returns:** `ok`
**Panics:** If called more than once.
**Auth:** None

---

#### `get_admin(env) → Result<Address, Error>`

Return the current admin.

**Errors:** `NotInitialized`
**Auth:** None (read-only)

---

### 4.2 Retention Policy

#### `set_retention_policy(env, admin, policy) → Result<Symbol, Error>`

Set the retention policy. Admin-only.

| Parameter | Type | Description |
|-----------|------|-------------|
| `policy` | `RetentionPolicy` | Retention limits |

**Auth:** `admin.require_auth()` + admin check

---

#### `get_retention_policy(env) → RetentionPolicy`

Read the current retention policy. Returns unbounded default if not configured.

**Auth:** None (read-only)

---

### 4.3 Logging

#### `log_event(env, actor, event_type, portfolio, permissions_flags, state_before, state_after, outcome, detail) → u64`

Append a single audit event. Returns the assigned sequence id.

| Parameter | Type | Description |
|-----------|------|-------------|
| `actor` | `Address` | Caller/signer |
| `event_type` | `AuditEventType` | Type of event |
| `portfolio` | `Symbol` | Portfolio/scope |
| `permissions_flags` | `u32` | Permission bitmask |
| `state_before` | `StateSnapshot` | State before event |
| `state_after` | `StateSnapshot` | State after event |
| `outcome` | `Symbol` | Outcome (e.g., `ok`) |
| `detail` | `String` | Human-readable detail |

**Returns:** Sequence id (1, 2, 3, …)
**Auth:** Caller is trusted to have authorized `actor`.

---

### 4.4 Querying

#### `query(env, q) → Vec<AuditLog>`

Read entries matching the supplied `LogQuery`. Sorted by `seq` ascending.

| Parameter | Type | Description |
|-----------|------|-------------|
| `q` | `LogQuery` | Filter set |

**Auth:** None (read-only)

---

### 4.5 Integrity Verification

#### `verify_integrity(env, expected_head) → bool`

Verify that the recorded chain head matches the expected hash.

**Returns:** `true` on match; `false` if tampered or empty.
**Auth:** None (read-only)

---

#### `integrity_head(env) → BytesN<32>`

Return the recorded chain head hash.

**Auth:** None (read-only)

---

#### `full_recompute_integrity(env) → bool`

Recompute the chain head from scratch and compare to stored value.

**Returns:** `true` if the chain is intact.
**Auth:** None (read-only)

---

### 4.6 Pruning

#### `prune_old(env, admin) → Result<u32, Error>`

Delete entries older than the retention policy. Admin-only.

**Returns:** Number of entries pruned.
**Errors:** `NoRetentionPolicy` if policy is unbounded.
**Auth:** `admin.require_auth()` + admin check

---

### 4.7 Export

#### `export_jsonl(env, q) → Vec<String>`

Export query results as JSON-Lines (one JSON object per entry).

**Auth:** None (read-only)

---

#### `export_csv(env, q) → Vec<String>`

Export query results as CSV (header row first).

**Auth:** None (read-only)

---

### 4.8 Errors

| Error | Value | Description |
|-------|-------|-------------|
| `NotInitialized` | 1 | Contract not initialized |
| `Unauthorized` | 2 | Caller is not admin |
| `InvalidEvent` | 3 | Invalid event type (reserved) |
| `NoRetentionPolicy` | 4 | Cannot prune without retention policy |

---

### 4.9 Types

#### `AuditEventType`

| Variant | Value | Description |
|---------|-------|-------------|
| `Rebalance` | 0 | Portfolio rebalanced |
| `Stake` | 1 | Stake deposit |
| `Unstake` | 2 | Normal unstake |
| `EmergencyUnstake` | 3 | Emergency unstake |
| `YieldAccrual` | 4 | Yield checkpointed |
| `Deposit` | 5 | External deposit |
| `Withdrawal` | 6 | External withdrawal |
| `ScheduleChange` | 7 | Schedule modified |
| `AdminAction` | 8 | Admin configuration |
| `Custom` | 99 | Catch-all |

#### `AuditLog`

| Field | Type | Description |
|-------|------|-------------|
| `seq` | `u64` | Sequence id |
| `timestamp` | `u64` | Ledger timestamp |
| `event_type` | `AuditEventType` | Event type |
| `actor` | `Address` | Event initiator |
| `permissions` | `u32` | Permission bitmask |
| `portfolio` | `Symbol` | Portfolio/scope |
| `state_before` | `StateSnapshot` | State before |
| `state_after` | `StateSnapshot` | State after |
| `outcome` | `Symbol` | Outcome |
| `detail` | `String` | Detail text |
| `hash` | `BytesN<32>` | SHA-256 chain hash |

#### `StateSnapshot`

| Field | Type | Description |
|-------|------|-------------|
| `fields` | `Vec<FieldEntry>` | Key-value pairs |

#### `FieldEntry`

| Field | Type | Description |
|-------|------|-------------|
| `key` | `Symbol` | Field name |
| `value` | `i128` | Field value |

#### `RetentionPolicy`

| Field | Type | Description |
|-------|------|-------------|
| `max_entries` | `u64` | Max entries to retain (0 = no cap) |
| `max_age_seconds` | `u64` | Max age in seconds (0 = no cap) |

#### `LogQuery`

| Field | Type | Description |
|-------|------|-------------|
| `from_ts` | `u64` | Inclusive lower timestamp bound |
| `to_ts` | `u64` | Inclusive upper timestamp bound |
| `event_type` | `Option<AuditEventType>` | Event type filter |
| `actor` | `Option<Address>` | Actor filter |
| `portfolio` | `Option<Symbol>` | Portfolio filter |
| `limit` | `u32` | Max results |
| `cursor` | `u64` | Reserved for pagination |

#### Permission Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `NONE` | `0` | No permissions |
| `STAKER` | `1` | Staker permission |
| `ADMIN` | `2` | Admin permission |
| `TREASURY` | `4` | Treasury permission |
| `SYSTEM` | `8` | System permission |
