#![no_std]
//! # AstraPort Staking Contract
//!
//! Manages asset staking together with an accurate, compounding **yield
//! calculation engine**. The yield engine is organized into focused modules:
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

use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Symbol};

pub mod apy;
pub mod compounding;
pub mod engine;
pub mod fixed_point;
pub mod projection;
pub mod records;

use crate::apy::APYCalculator;
use crate::engine::YieldEngine;
use crate::fixed_point::SCALE;
use crate::projection::YieldProjector;
use crate::records::{
    CompoundingMode, DistributionSchedule, StakeDataKey, StakingConfig, YieldHistoryEntry,
    YieldProjection, YieldRecord,
};

/// Default APR seeded onto a newly opened yield position: 5% (fixed-point).
const DEFAULT_APR: i128 = SCALE / 20;
/// Default compounding mode seeded onto a newly opened yield position.
const DEFAULT_MODE: CompoundingMode = CompoundingMode::Daily;

/// Staking contract for AstraPort
/// Manages staking operations, alert functionality, and yield calculation.
#[contract]
pub struct StakingContract;

#[contractimpl]
impl StakingContract {
    /// Initialize the staking contract
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// Success symbol if initialization succeeds
    pub fn initialize(_env: Env) -> Symbol {
        symbol_short!("ok")
    }

    /// Stake `amount` of `asset` for `staker`, wiring the deposit into the yield
    /// engine so the position's principal always equals the staked balance.
    ///
    /// If this is the first stake for the pair, a yield position is opened at the
    /// configured default APR / compounding mode (see [`Self::set_yield_defaults`]).
    /// If a position already exists, it is checkpointed (accrued to now) and its
    /// principal raised to the new balance, preserving both its rate/mode and any
    /// realized yield.
    ///
    /// # Returns
    /// The staker's new staked balance for the asset.
    pub fn stake(env: Env, staker: Address, asset: Symbol, amount: i128) -> i128 {
        if amount <= 0 {
            panic!("stake amount must be positive");
        }
        let new_balance = Self::balance_of(&env, &staker, &asset)
            .checked_add(amount)
            .expect("stake balance overflow");

        let engine = YieldEngine::new(&env);
        match engine.load_record(&staker, &asset) {
            // Existing position: checkpoint, then raise principal (keeps its rate).
            Some(_) => {
                engine
                    .set_principal(&staker, &asset, new_balance)
                    .expect("failed to adjust yield principal on stake");
            }
            // First stake: open a position seeded with the default parameters.
            None => {
                let config = Self::load_config(&env);
                engine
                    .open_position(
                        &staker,
                        &asset,
                        new_balance,
                        config.default_apr,
                        config.default_mode,
                    )
                    .expect("failed to open yield position on stake");
            }
        }

        Self::set_balance(&env, &staker, &asset, new_balance);
        new_balance
    }

    /// Unstake `amount` of `asset` for `staker`, checkpointing accrued yield
    /// before reducing the position's principal so no earned yield is lost.
    ///
    /// # Returns
    /// The staker's remaining staked balance for the asset.
    pub fn unstake(env: Env, staker: Address, asset: Symbol, amount: i128) -> i128 {
        let current = Self::balance_of(&env, &staker, &asset);
        if amount <= 0 || amount > current {
            panic!("invalid unstake amount");
        }
        let new_balance = current - amount;

        // set_principal accrues (checkpoints) at the current rate *before* the
        // principal drops, so realized yield is preserved.
        YieldEngine::new(&env)
            .set_principal(&staker, &asset, new_balance)
            .expect("failed to adjust yield principal on unstake");

        Self::set_balance(&env, &staker, &asset, new_balance);
        new_balance
    }

    /// Get the staked balance for a `(staker, asset)` pair.
    pub fn get_balance(env: Env, staker: Address, asset: Symbol) -> i128 {
        Self::balance_of(&env, &staker, &asset)
    }

    /// Configure the default APR and compounding mode used when a new yield
    /// position is opened by the first [`Self::stake`] for a pair.
    ///
    /// Existing positions are unaffected; change an open position's rate with
    /// [`Self::set_yield_rate`] instead.
    ///
    /// # Returns
    /// The stored [`StakingConfig`].
    pub fn set_yield_defaults(env: Env, apr: i128, mode: CompoundingMode) -> StakingConfig {
        let config = StakingConfig {
            default_apr: apr,
            default_mode: mode,
        };
        env.storage()
            .persistent()
            .set(&StakeDataKey::Config, &config);
        config
    }

    /// Set alert threshold for staking changes
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `threshold` - Alert threshold amount
    ///
    /// # Returns
    /// Success symbol if alert threshold is set
    pub fn set_alert_threshold(_env: Env, _threshold: i128) -> Symbol {
        symbol_short!("ok")
    }

    // ---------------------------------------------------------------------
    // Yield calculation engine entrypoints
    // ---------------------------------------------------------------------

    /// Open (or reset) a yield-accruing position for a staker and asset.
    ///
    /// Starts accruing yield from the current ledger time at the given `apr`
    /// (fixed-point, e.g. `50_000_000_000_000_000` for 5%) under the chosen
    /// compounding `mode`. If a position already exists it is checkpointed and
    /// its accrued yield preserved.
    ///
    /// # Returns
    /// The active [`YieldRecord`].
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

    /// Checkpoint a position, realizing all yield accrued up to the current
    /// ledger time and recording a history entry.
    ///
    /// # Returns
    /// The updated [`YieldRecord`].
    pub fn accrue_yield(env: Env, staker: Address, asset: Symbol) -> YieldRecord {
        YieldEngine::new(&env)
            .accrue(&staker, &asset)
            .expect("failed to accrue yield")
    }

    /// The total yield a position has earned as of now (checkpointed plus
    /// pending), without mutating storage.
    pub fn current_yield(env: Env, staker: Address, asset: Symbol) -> i128 {
        YieldEngine::new(&env)
            .current_yield(&staker, &asset)
            .expect("failed to read current yield")
    }

    /// Change the APR for a position, checkpointing prior yield at the old rate
    /// first so the transition is time-weighted and exact.
    ///
    /// # Returns
    /// The updated [`YieldRecord`] carrying the new rate.
    pub fn set_yield_rate(env: Env, staker: Address, asset: Symbol, new_apr: i128) -> YieldRecord {
        YieldEngine::new(&env)
            .set_rate(&staker, &asset, new_apr)
            .expect("failed to set yield rate")
    }

    /// The complete, queryable yield history for a staker/asset pair, oldest
    /// entry first.
    pub fn yield_history(
        env: Env,
        staker: Address,
        asset: Symbol,
    ) -> soroban_sdk::Vec<YieldHistoryEntry> {
        YieldEngine::new(&env).history(&staker, &asset)
    }

    /// Project future earnings for a set of position parameters over a horizon.
    ///
    /// This is a pure calculation — it does not require an existing position and
    /// does not touch storage — so it can be used to preview any scenario.
    ///
    /// # Arguments
    /// * `horizon_seconds` - How far into the future to project.
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
    ///
    /// Both values are fixed-point fractions (see [`fixed_point::SCALE`]).
    pub fn apr_to_apy(_env: Env, apr: i128, mode: CompoundingMode) -> i128 {
        APYCalculator::apr_to_apy(apr, mode.to_strategy()).expect("apr_to_apy failed")
    }

    /// Convert an effective APY back to its nominal APR under a compounding mode.
    pub fn apy_to_apr(_env: Env, apy: i128, mode: CompoundingMode) -> i128 {
        APYCalculator::apy_to_apr(apy, mode.to_strategy()).expect("apy_to_apr failed")
    }

    /// Schedule a yield distribution to a staker.
    ///
    /// `interval_seconds` of 0 schedules a one-off distribution; a positive
    /// interval makes it recurring.
    ///
    /// # Returns
    /// The created [`DistributionSchedule`].
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

    /// Process due distributions for a staker/asset as of the current ledger
    /// time, returning the total amount that became due.
    pub fn process_distribution(env: Env, staker: Address, asset: Symbol) -> i128 {
        YieldEngine::new(&env).process_distribution(&staker, &asset)
    }
}

/// Internal helpers (not part of the contract's external interface).
impl StakingContract {
    /// Read the staked balance for a pair, defaulting to `0`.
    fn balance_of(env: &Env, staker: &Address, asset: &Symbol) -> i128 {
        env.storage()
            .persistent()
            .get(&StakeDataKey::Balance(staker.clone(), asset.clone()))
            .unwrap_or(0)
    }

    /// Persist the staked balance for a pair.
    fn set_balance(env: &Env, staker: &Address, asset: &Symbol, balance: i128) {
        env.storage().persistent().set(
            &StakeDataKey::Balance(staker.clone(), asset.clone()),
            &balance,
        );
    }

    /// Load the configured yield defaults, falling back to the built-in
    /// [`DEFAULT_APR`] / [`DEFAULT_MODE`] when unset.
    fn load_config(env: &Env) -> StakingConfig {
        env.storage()
            .persistent()
            .get(&StakeDataKey::Config)
            .unwrap_or(StakingConfig {
                default_apr: DEFAULT_APR,
                default_mode: DEFAULT_MODE,
            })
    }
}

#[cfg(test)]
mod tests;
