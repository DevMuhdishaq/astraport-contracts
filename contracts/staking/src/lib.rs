#![no_std]
//! # AstraPort Staking Contract
//!
//! Manages asset staking together with an accurate, compounding **yield
//! calculation engine** and a **configurable alert monitoring system**.
//!
//! ## Modules
//!
//! - [`fixed_point`] — deterministic fixed-point math (`mul`, `div`, `pow`,
//!   `exp`, `ln`) used in place of floating point.
//! - [`compounding`] — the [`compounding::CompoundingStrategy`] trait with
//!   `Daily` and `Continuous` variants, plus the [`compounding::YieldCalculator`].
//! - [`apy`] — [`apy::APYCalculator`] for accurate APR ⇄ APY conversion.
//! - [`records`] — Soroban-typed [`records::YieldRecord`],
//!   [`records::YieldHistoryEntry`], [`records::YieldProjection`], and
//!   [`records::DistributionSchedule`].
//! - [`engine`] — the storage-backed [`engine::YieldEngine`] that performs
//!   real-time accrual, time-weighted rate changes, history logging, and
//!   distribution scheduling.
//! - [`projection`] — [`projection::YieldProjector`] for future-earnings
//!   estimates.
//! - [`alerts`] — [`alerts::AlertMonitor`] with threshold-based alerting for
//!   balance drops, yield underperformance, upcoming unlocks, and custom
//!   conditions, plus full history and acknowledgment support.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, String,
    Symbol, Vec,
};

pub mod alerts;
pub mod apy;
pub mod compounding;
pub mod engine;
pub mod fixed_point;
pub mod projection;
pub mod records;

use crate::alerts::{AlertConfig, AlertHistoryEntry, AlertMonitor, AlertThreshold};
use crate::apy::APYCalculator;
use crate::engine::YieldEngine;
use crate::fixed_point::SCALE;
use crate::projection::YieldProjector;
use crate::records::{
    CompoundingMode, DistributionSchedule, StakeDataKey, YieldDataKey, YieldHistoryEntry,
    YieldProjection, YieldRecord, StakingConfig,
};

// ---------------------------------------------------------------------------
// Contract-level constants
// ---------------------------------------------------------------------------

/// Default APR applied to new yield positions: 5% (0.05 × SCALE).
const DEFAULT_APR: i128 = SCALE / 20;

/// Default compounding mode applied to new yield positions.
const DEFAULT_MODE: CompoundingMode = CompoundingMode::Daily;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by the staking contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// Invalid amount: must be positive for staking.
    InvalidStakeAmount = 1,
    /// Insufficient balance: cannot unstake more than currently staked.
    InsufficientBalance = 2,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Event emitted when assets are staked.
#[contracttype]
#[derive(Debug, Clone)]
pub struct StakeEvent {
    pub staker: Address,
    pub amount: i128,
    pub new_balance: i128,
}

/// Event emitted when assets are unstaked.
#[contracttype]
#[derive(Debug, Clone)]
pub struct UnstakeEvent {
    pub staker: Address,
    pub amount: i128,
    pub new_balance: i128,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

/// Staking contract for AstraPort.
///
/// Manages staking operations, yield calculation, and the configurable alert
/// monitoring system.
#[contract]
pub struct StakingContract;

#[contractimpl]
impl StakingContract {
    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// Initialize the staking contract with an admin.
    ///
    /// Can only be called once; subsequent calls will panic.
    pub fn initialize(env: Env, admin: Address) -> Symbol {
        let storage = env.storage().persistent();
        if storage.has(&YieldDataKey::Admin) {
            panic!("already initialized");
        }
        storage.set(&YieldDataKey::Admin, &admin);
        symbol_short!("ok")
    }

    // -----------------------------------------------------------------------
    // Staking
    // -----------------------------------------------------------------------

    /// Stake assets into the contract.
    ///
    /// Requires authorization from the staker. If this is the first stake the
    /// balance is created; otherwise it is incremented.
    ///
    /// Returns `"done"` on success.
    pub fn stake(env: Env, staker: Address, amount: i128) -> Result<Symbol, Error> {
        staker.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidStakeAmount);
        }
        let key = StakeDataKey::Balance(staker.clone());
        let current_balance: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_balance = current_balance + amount;
        env.storage().persistent().set(&key, &new_balance);
        env.events().publish(
            (symbol_short!("stake"), staker.clone()),
            StakeEvent {
                staker,
                amount,
                new_balance,
            },
        );
        Ok(symbol_short!("done"))
    }

    /// Unstake assets from the contract.
    ///
    /// Requires authorization from the staker.
    ///
    /// Returns `"done"` on success.
    pub fn unstake(env: Env, staker: Address, amount: i128) -> Result<Symbol, Error> {
        staker.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidStakeAmount);
        }
        let key = StakeDataKey::Balance(staker.clone());
        let current_balance: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if amount > current_balance {
            return Err(Error::InsufficientBalance);
        }
        let new_balance = current_balance - amount;
        if new_balance == 0 {
            env.storage().persistent().remove(&key);
        } else {
            env.storage().persistent().set(&key, &new_balance);
        }
        env.events().publish(
            (symbol_short!("unstake"), staker.clone()),
            UnstakeEvent {
                staker,
                amount,
                new_balance,
            },
        );
        Ok(symbol_short!("done"))
    }

    /// Get staking balance for an address.
    pub fn get_balance(env: Env, staker: Address) -> i128 {
        let key = StakeDataKey::Balance(staker);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    // -----------------------------------------------------------------------
    // Legacy simple alert threshold (admin-controlled global value)
    // -----------------------------------------------------------------------

    /// Set the global alert threshold.
    ///
    /// Only callable by the admin set during [`Self::initialize`].
    pub fn set_alert_threshold(env: Env, admin: Address, threshold: i128) -> Symbol {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .persistent()
            .get(&YieldDataKey::Admin)
            .expect("contract not initialized");
        assert!(stored_admin == admin, "caller is not admin");
        env.storage()
            .persistent()
            .set(&YieldDataKey::AlertThreshold, &threshold);
        symbol_short!("ok")
    }

    // -----------------------------------------------------------------------
    // Alert preference management
    // -----------------------------------------------------------------------

    /// Create or fully replace the alert configuration for a `(staker, asset)` pair.
    ///
    /// Requires authorization from `staker`. The supplied `thresholds` vector
    /// must not exceed [`alerts::MAX_THRESHOLDS_PER_CONFIG`] entries.
    ///
    /// Returns the stored [`AlertConfig`].
    pub fn set_alert_config(
        env: Env,
        staker: Address,
        asset: Symbol,
        thresholds: Vec<AlertThreshold>,
        alerts_enabled: bool,
    ) -> AlertConfig {
        staker.require_auth();
        assert!(
            thresholds.len() <= crate::alerts::MAX_THRESHOLDS_PER_CONFIG,
            "too many thresholds"
        );
        let config = AlertConfig {
            staker: staker.clone(),
            asset: asset.clone(),
            thresholds,
            alerts_enabled,
        };
        AlertMonitor::new(&env).set_config(config)
    }

    /// Retrieve the alert configuration for a `(staker, asset)` pair.
    ///
    /// Returns `None` when no configuration has been created yet.
    pub fn get_alert_config(env: Env, staker: Address, asset: Symbol) -> Option<AlertConfig> {
        AlertMonitor::new(&env).get_config(&staker, &asset)
    }

    /// Append a single threshold to an existing alert config.
    ///
    /// Requires authorization from `staker`. Panics if no config exists for the
    /// pair or the threshold limit would be exceeded.
    pub fn add_alert_threshold(
        env: Env,
        staker: Address,
        asset: Symbol,
        threshold: AlertThreshold,
    ) -> AlertConfig {
        staker.require_auth();
        AlertMonitor::new(&env).add_threshold(&staker, &asset, threshold)
    }

    /// Remove the threshold at the given `index` (0-based) from a config.
    ///
    /// Requires authorization from `staker`.
    pub fn remove_alert_threshold(
        env: Env,
        staker: Address,
        asset: Symbol,
        index: u32,
    ) -> AlertConfig {
        staker.require_auth();
        AlertMonitor::new(&env).remove_threshold(&staker, &asset, index)
    }

    /// Enable or disable all alert evaluation for a `(staker, asset)` pair.
    ///
    /// Requires authorization from `staker`.
    pub fn set_alerts_enabled(
        env: Env,
        staker: Address,
        asset: Symbol,
        enabled: bool,
    ) -> AlertConfig {
        staker.require_auth();
        AlertMonitor::new(&env).set_alerts_enabled(&staker, &asset, enabled)
    }

    // -----------------------------------------------------------------------
    // Alert monitoring
    // -----------------------------------------------------------------------

    /// Evaluate all enabled thresholds for a `(staker, asset)` pair.
    ///
    /// Fires [`alerts::AlertEvent`] Soroban events and appends
    /// [`AlertHistoryEntry`] records for every breached threshold. Typically
    /// called after a stake/unstake or yield accrual to surface relevant alerts.
    ///
    /// `unlock_ts` — optional lock-up expiry in ledger seconds; pass `0` when
    /// there is no lock-up (unlock-date thresholds are skipped).
    ///
    /// Returns the count of alerts that fired.
    pub fn check_alerts(
        env: Env,
        staker: Address,
        asset: Symbol,
        current_balance: i128,
        current_apr: i128,
        unlock_ts: u64,
    ) -> u32 {
        AlertMonitor::new(&env).check(
            &staker,
            &asset,
            current_balance,
            current_apr,
            unlock_ts,
        )
    }

    // -----------------------------------------------------------------------
    // Alert history and acknowledgment
    // -----------------------------------------------------------------------

    /// Return the full alert history for a `(staker, asset)` pair, oldest first.
    pub fn alert_history(
        env: Env,
        staker: Address,
        asset: Symbol,
    ) -> Vec<AlertHistoryEntry> {
        AlertMonitor::new(&env).history(&staker, &asset)
    }

    /// Return only unacknowledged alerts for a `(staker, asset)` pair.
    pub fn pending_alerts(
        env: Env,
        staker: Address,
        asset: Symbol,
    ) -> Vec<AlertHistoryEntry> {
        AlertMonitor::new(&env).pending_alerts(&staker, &asset)
    }

    /// Acknowledge the alert at `index` in the history log.
    ///
    /// Requires authorization from `staker`. The entry is retained for audit;
    /// only the `acknowledged` flag is set to `true`.
    pub fn acknowledge_alert(env: Env, staker: Address, asset: Symbol, index: u32) {
        staker.require_auth();
        AlertMonitor::new(&env).acknowledge(&staker, &asset, index);
    }

    // -----------------------------------------------------------------------
    // Yield calculation engine
    // -----------------------------------------------------------------------

    /// Open (or reset) a yield-accruing position for a staker and asset.
    ///
    /// Starts accruing yield from the current ledger time at the given `apr`
    /// (fixed-point) under the chosen compounding `mode`. If a position already
    /// exists it is checkpointed and its accrued yield preserved.
    pub fn open_yield_position(
        env: Env,
        staker: Address,
        asset: Symbol,
        principal: i128,
        apr: i128,
        mode: CompoundingMode,
    ) -> YieldRecord {
        YieldEngine::new(&env)
            .open_position(&staker, &asset, principal, apr, mode)
            .expect("failed to open yield position")
    }

    /// Checkpoint a position, realizing all yield accrued up to now.
    pub fn accrue_yield(env: Env, staker: Address, asset: Symbol) -> YieldRecord {
        YieldEngine::new(&env)
            .accrue(&staker, &asset)
            .expect("failed to accrue yield")
    }

    /// Claim all yield accrued by a staker for an asset.
    ///
    /// The position is checkpointed first; the full amount is returned and the
    /// unclaimed counter is reset to zero.
    pub fn claim_yield(env: Env, staker: Address, asset: Symbol) -> i128 {
        staker.require_auth();
        let engine = YieldEngine::new(&env);
        let record = engine
            .accrue(&staker, &asset)
            .expect("failed to accrue yield before claim");
        let claimed = engine.finalize_claim(record);
        env.events()
            .publish((symbol_short!("YLDCLAIM"), staker, asset), claimed);
        claimed
    }

    /// The total yield a position has earned as of now, without mutating storage.
    pub fn current_yield(env: Env, staker: Address, asset: Symbol) -> i128 {
        YieldEngine::new(&env)
            .current_yield(&staker, &asset)
            .expect("failed to read current yield")
    }

    /// Change the APR for a position, checkpointing prior yield at the old rate first.
    pub fn set_yield_rate(env: Env, staker: Address, asset: Symbol, new_apr: i128) -> YieldRecord {
        YieldEngine::new(&env)
            .set_rate(&staker, &asset, new_apr)
            .expect("failed to set yield rate")
    }

    /// The complete yield history for a staker/asset pair, oldest entry first.
    pub fn yield_history(
        env: Env,
        staker: Address,
        asset: Symbol,
    ) -> Vec<YieldHistoryEntry> {
        YieldEngine::new(&env).history(&staker, &asset)
    }

    /// Project future earnings for a set of position parameters over a horizon.
    ///
    /// Pure calculation — does not touch storage.
    pub fn project_yield(
        _env: Env,
        principal: i128,
        apr: i128,
        mode: CompoundingMode,
        horizon_seconds: u64,
    ) -> YieldProjection {
        YieldProjector::project(principal, apr, mode, horizon_seconds)
            .expect("failed to project yield")
    }

    /// Convert a nominal APR to its effective APY under a compounding mode.
    pub fn apr_to_apy(_env: Env, apr: i128, mode: CompoundingMode) -> i128 {
        APYCalculator::apr_to_apy(apr, mode.to_strategy()).expect("apr_to_apy failed")
    }

    /// Convert an effective APY back to its nominal APR under a compounding mode.
    pub fn apy_to_apr(_env: Env, apy: i128, mode: CompoundingMode) -> i128 {
        APYCalculator::apy_to_apr(apy, mode.to_strategy()).expect("apy_to_apr failed")
    }

    /// Schedule a yield distribution to a staker.
    ///
    /// `interval_seconds` of 0 schedules a one-off; a positive interval makes
    /// it recurring.
    pub fn schedule_distribution(
        env: Env,
        staker: Address,
        asset: Symbol,
        amount: i128,
        due_ts: u64,
        interval_seconds: u64,
    ) -> DistributionSchedule {
        YieldEngine::new(&env).schedule_distribution(
            &staker,
            &asset,
            amount,
            due_ts,
            interval_seconds,
        )
    }

    /// Process due distributions for a staker/asset as of the current ledger time.
    pub fn process_distribution(env: Env, staker: Address, asset: Symbol) -> i128 {
        YieldEngine::new(&env).process_distribution(&staker, &asset)
    }
}

#[cfg(test)]
mod tests;
