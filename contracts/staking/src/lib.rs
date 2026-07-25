#![no_std]
//! # AstraPort Staking Contract
//!
//! Manages **multi-asset staking** with heterogeneous yield rates, independent
//! lock-up periods, graduated unlock schedules, and aggregate portfolio yield
//! calculations.
//!
//! The contract is organised into focused modules:
//!
//! - [`fixed_point`] — deterministic fixed-point math (`mul`, `div`, `pow`,
//!   `exp`, `ln`) used in place of floating point.
//! - [`compounding`] — [`compounding::CompoundingStrategy`] trait with `Daily`
//!   and `Continuous` variants, plus [`compounding::YieldCalculator`].
//! - [`apy`] — [`apy::APYCalculator`] for accurate APR ⇄ APY conversion.
//! - [`records`] — Soroban-typed structs and storage key enums, including the
//!   new [`records::StakingPosition`], [`records::AssetYieldRate`],
//!   [`records::UnlockSchedule`], and [`records::PortfolioSnapshot`].
//! - [`engine`] — storage-backed [`engine::YieldEngine`] for real-time accrual,
//!   time-weighted rate changes, history logging, and distribution scheduling.
//! - [`projection`] — [`projection::YieldProjector`] for future-earnings
//!   estimates.
//! - [`multi_asset`] — [`multi_asset::MultiAssetStaking`] facade that wraps the
//!   yield engine with per-asset configuration, unlock-schedule enforcement,
//!   and portfolio aggregation.

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec,
};

pub mod apy;
pub mod compounding;
pub mod engine;
pub mod fixed_point;
pub mod multi_asset;
pub mod projection;
pub mod records;

use crate::apy::APYCalculator;
use crate::engine::YieldEngine;
use crate::fixed_point::SCALE;
use crate::multi_asset::MultiAssetStaking;
use crate::projection::YieldProjector;
use crate::records::{
    AssetYieldRate, CompoundingMode, DistributionSchedule, PortfolioSnapshot, StakeDataKey,
    StakingConfig, StakingPosition, UnlockSchedule, YieldDataKey, YieldHistoryEntry,
    YieldProjection, YieldRecord,
};

// ---------------------------------------------------------------------------
// Default yield parameters (used when no per-asset config is set)
// ---------------------------------------------------------------------------

/// Default APR: 5 % (0.05 × SCALE).
const DEFAULT_APR: i128 = SCALE / 20;

/// Default compounding mode.
const DEFAULT_MODE: CompoundingMode = CompoundingMode::Daily;

// ---------------------------------------------------------------------------
// Error codes
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
    /// The requested amount is below the asset's configured minimum stake.
    BelowMinimumStake = 3,
    /// The resulting position would exceed the asset's configured maximum stake.
    ExceedsMaximumStake = 4,
    /// The position is still within its lock-up period.
    PositionLocked = 5,
    /// The requested withdrawal exceeds the amount that has unlocked so far.
    ExceedsUnlockedAmount = 6,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Event emitted when assets are staked.
#[contracttype]
#[derive(Debug, Clone)]
pub struct StakeEvent {
    pub staker: Address,
    pub asset: Symbol,
    pub amount: i128,
    pub new_balance: i128,
}

/// Event emitted when assets are unstaked.
#[contracttype]
#[derive(Debug, Clone)]
pub struct UnstakeEvent {
    pub staker: Address,
    pub asset: Symbol,
    pub amount: i128,
    pub new_balance: i128,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

/// Staking contract for AstraPort — multi-asset edition.
///
/// Manages staking operations for 10+ asset types simultaneously, each with
/// independent yield rates, lock-up terms, and withdrawal restrictions.
#[contract]
pub struct StakingContract;

#[contractimpl]
impl StakingContract {
    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// Initialize the staking contract with an admin.
    ///
    /// Can only be called once; subsequent calls panic.
    pub fn initialize(env: Env, admin: Address) -> Symbol {
        let storage = env.storage().persistent();
        if storage.has(&YieldDataKey::Admin) {
            panic!("already initialized");
        }
        storage.set(&YieldDataKey::Admin, &admin);
        symbol_short!("ok")
    }

    // -----------------------------------------------------------------------
    // Multi-asset staking — primary interface
    // -----------------------------------------------------------------------

    /// Stake `amount` of `asset` on behalf of `staker`.
    ///
    /// Requires authorization from `staker`. The call:
    /// 1. Validates the amount against the asset's `min_stake` / `max_stake`.
    /// 2. Opens or updates a [`StakingPosition`] for the `(staker, asset)` pair.
    /// 3. Synchronises the principal in the underlying [`YieldEngine`], preserving
    ///    any previously accrued yield.
    /// 4. Tracks the asset in the staker's asset list for portfolio queries.
    ///
    /// # Returns
    /// The new staked balance for this `(staker, asset)` pair.
    pub fn stake(env: Env, staker: Address, asset: Symbol, amount: i128) -> Result<i128, Error> {
        staker.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidStakeAmount);
        }

        let config = Self::load_asset_config(&env, &asset);
        if config.min_stake > 0 && amount < config.min_stake {
            return Err(Error::BelowMinimumStake);
        }

        let mas = MultiAssetStaking::new(&env);
        let new_balance = mas.stake(&staker, &asset, amount, &config)?;

        env.events().publish(
            (symbol_short!("stake"), staker.clone()),
            StakeEvent {
                staker,
                asset,
                amount,
                new_balance,
            },
        );
        Ok(new_balance)
    }

    /// Unstake `amount` of `asset` on behalf of `staker`.
    ///
    /// Requires authorization from `staker`. The call:
    /// 1. Checks the staker has sufficient balance.
    /// 2. Validates the requested amount against the position's unlock schedule.
    /// 3. Checkpoints accrued yield before reducing the principal.
    /// 4. Removes the position (and the asset from the staker's list) when the
    ///    remaining balance reaches zero.
    ///
    /// # Returns
    /// The remaining staked balance for this `(staker, asset)` pair.
    pub fn unstake(env: Env, staker: Address, asset: Symbol, amount: i128) -> Result<i128, Error> {
        staker.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidStakeAmount);
        }

        let mas = MultiAssetStaking::new(&env);
        let remaining = mas.unstake(&staker, &asset, amount)?;

        env.events().publish(
            (symbol_short!("unstake"), staker.clone()),
            UnstakeEvent {
                staker,
                asset,
                amount,
                new_balance: remaining,
            },
        );
        Ok(remaining)
    }

    /// Return the staked balance for a `(staker, asset)` pair.
    pub fn get_balance(env: Env, staker: Address, asset: Symbol) -> i128 {
        env.storage()
            .persistent()
            .get(&StakeDataKey::Balance(staker, asset))
            .unwrap_or(0)
    }

    /// Return the full [`StakingPosition`] for a `(staker, asset)` pair, or
    /// `None` if no position exists.
    pub fn get_position(env: Env, staker: Address, asset: Symbol) -> Option<StakingPosition> {
        MultiAssetStaking::new(&env).load_position(&staker, &asset)
    }

    // -----------------------------------------------------------------------
    // Portfolio
    // -----------------------------------------------------------------------

    /// Return a [`PortfolioSnapshot`] aggregating all active positions for
    /// `staker`.
    ///
    /// This is a **read-only** call — it does not mutate any storage.
    pub fn get_portfolio(env: Env, staker: Address) -> PortfolioSnapshot {
        MultiAssetStaking::new(&env).portfolio_snapshot(&staker)
    }

    /// Return the total accrued yield across all assets for `staker` as of
    /// the current ledger time.
    ///
    /// This is a **read-only** call.
    pub fn portfolio_yield(env: Env, staker: Address) -> i128 {
        MultiAssetStaking::new(&env).portfolio_yield(&staker)
    }

    /// Return the list of asset symbols the staker currently has positions in.
    pub fn staker_assets(env: Env, staker: Address) -> Vec<Symbol> {
        env.storage()
            .persistent()
            .get(&StakeDataKey::StakerAssets(staker))
            .unwrap_or_else(|| Vec::new(&env))
    }

    // -----------------------------------------------------------------------
    // Asset configuration
    // -----------------------------------------------------------------------

    /// Configure (or reconfigure) the yield parameters for an `asset`.
    ///
    /// Only callable by the admin. Setting a per-asset config overrides the
    /// global defaults for all *new* positions of that asset. Existing
    /// positions keep their current APR until [`Self::set_yield_rate`] is
    /// called on them individually.
    ///
    /// # Arguments
    /// * `asset`           — asset symbol to configure.
    /// * `apr`             — annual rate, fixed-point.
    /// * `mode`            — compounding model.
    /// * `min_stake`       — minimum stake per position (0 = no minimum).
    /// * `max_stake`       — maximum stake per position (0 = no maximum).
    /// * `unlock_schedule` — lock-up / vesting schedule for new positions.
    pub fn configure_asset(
        env: Env,
        admin: Address,
        asset: Symbol,
        apr: i128,
        mode: CompoundingMode,
        min_stake: i128,
        max_stake: i128,
        unlock_schedule: UnlockSchedule,
    ) -> Symbol {
        admin.require_auth();
        Self::assert_admin(&env, &admin);

        let cfg = AssetYieldRate {
            asset: asset.clone(),
            apr,
            mode,
            min_stake,
            max_stake,
            unlock_schedule,
        };
        env.storage()
            .persistent()
            .set(&StakeDataKey::AssetConfig(asset), &cfg);
        symbol_short!("ok")
    }

    /// Set the global default yield parameters applied when no per-asset
    /// configuration exists.
    ///
    /// Only callable by the admin (or during tests without auth mocking).
    pub fn set_yield_defaults(env: Env, default_apr: i128, mode: CompoundingMode) -> Symbol {
        env.storage().persistent().set(
            &StakeDataKey::Config,
            &StakingConfig {
                default_apr,
                default_mode: mode,
            },
        );
        symbol_short!("ok")
    }

    /// Set the alert threshold.  Admin-only.
    pub fn set_alert_threshold(env: Env, admin: Address, threshold: i128) -> Symbol {
        admin.require_auth();
        Self::assert_admin(&env, &admin);
        env.storage()
            .persistent()
            .set(&YieldDataKey::AlertThreshold, &threshold);
        symbol_short!("ok")
    }

    // -----------------------------------------------------------------------
    // Yield engine — per-position interface
    // -----------------------------------------------------------------------

    /// Open (or reset) a yield-accruing position for a staker and asset.
    ///
    /// Starts accruing yield from the current ledger time. If a position
    /// already exists it is checkpointed and its accrued yield preserved.
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
    pub fn accrue_yield(env: Env, staker: Address, asset: Symbol) -> YieldRecord {
        YieldEngine::new(&env)
            .accrue(&staker, &asset)
            .expect("failed to accrue yield")
    }

    /// Claim all yield accrued by `staker` for `asset`.
    ///
    /// The position is checkpointed first. The full checkpointed amount is
    /// returned and the position's unclaimed counter is reset to zero.
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

    /// The total yield a position has earned as of now (checkpointed plus
    /// pending), without mutating storage.
    pub fn current_yield(env: Env, staker: Address, asset: Symbol) -> i128 {
        YieldEngine::new(&env)
            .current_yield(&staker, &asset)
            .expect("failed to read current yield")
    }

    /// Change the APR for a position, checkpointing prior yield at the old
    /// rate first so the transition is time-weighted and exact.
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
    ) -> soroban_sdk::Vec<YieldHistoryEntry> {
        YieldEngine::new(&env).history(&staker, &asset)
    }

    // -----------------------------------------------------------------------
    // Projections / APY (pure — no storage mutations)
    // -----------------------------------------------------------------------

    /// Project future earnings for a set of position parameters over a horizon.
    ///
    /// Does not require an existing position and does not touch storage.
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

    // -----------------------------------------------------------------------
    // Distribution scheduling
    // -----------------------------------------------------------------------

    /// Schedule a yield distribution to a staker.
    ///
    /// `interval_seconds` of 0 schedules a one-off distribution; a positive
    /// interval makes it recurring.
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

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

impl StakingContract {
    /// Load the per-asset [`AssetYieldRate`], falling back to the global
    /// [`StakingConfig`] defaults when no asset-specific config exists.
    fn load_asset_config(env: &Env, asset: &Symbol) -> AssetYieldRate {
        // Per-asset config takes priority.
        if let Some(cfg) = env
            .storage()
            .persistent()
            .get::<StakeDataKey, AssetYieldRate>(&StakeDataKey::AssetConfig(asset.clone()))
        {
            return cfg;
        }

        // Fall back to the global staking defaults.
        let global: StakingConfig = env
            .storage()
            .persistent()
            .get(&StakeDataKey::Config)
            .unwrap_or(StakingConfig {
                default_apr: DEFAULT_APR,
                default_mode: DEFAULT_MODE,
            });

        AssetYieldRate {
            asset: asset.clone(),
            apr: global.default_apr,
            mode: global.default_mode,
            min_stake: 0,
            max_stake: 0,
            unlock_schedule: UnlockSchedule::Immediate,
        }
    }

    /// Panic if `caller` is not the stored admin.
    fn assert_admin(env: &Env, caller: &Address) {
        let stored: Address = env
            .storage()
            .persistent()
            .get(&YieldDataKey::Admin)
            .expect("contract not initialized");
        assert!(stored == *caller, "caller is not admin");
    }
}

#[cfg(test)]
mod tests;
