//! Soroban-typed records for tracking yield accrual, history, and schedules.
//!
//! These types are marked `#[contracttype]` so they can be persisted in
//! contract storage and returned across the contract boundary. They wrap the
//! pure-math results from [`crate::compounding`] and [`crate::apy`] into durable,
//! queryable structures keyed by staker and asset.

use soroban_sdk::{contracttype, Address, Symbol, Vec};

/// The compounding model, mirrored as a `#[contracttype]` for storage.
///
/// [`crate::compounding::Compounding`] is the pure-Rust enum used by the math
/// layer; this is its serializable twin used at the contract boundary. Convert
/// between them with [`CompoundingMode::to_strategy`] and [`CompoundingMode::from_strategy`].
#[contracttype]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundingMode {
    /// Daily compounding (365 periods/year).
    Daily,
    /// Continuous compounding (`e^(rt)`).
    Continuous,
}

impl CompoundingMode {
    /// Convert to the pure-math [`crate::compounding::Compounding`] strategy.
    pub fn to_strategy(self) -> crate::compounding::Compounding {
        match self {
            CompoundingMode::Daily => crate::compounding::Compounding::Daily,
            CompoundingMode::Continuous => crate::compounding::Compounding::Continuous,
        }
    }

    /// Build from the pure-math [`crate::compounding::Compounding`] strategy.
    pub fn from_strategy(s: crate::compounding::Compounding) -> Self {
        match s {
            crate::compounding::Compounding::Daily => CompoundingMode::Daily,
            crate::compounding::Compounding::Continuous => CompoundingMode::Continuous,
        }
    }
}

// ---------------------------------------------------------------------------
// Core yield records
// ---------------------------------------------------------------------------

/// A staker's active yield-accruing position for a single asset.
///
/// `accrued_yield` is the yield already realized and checkpointed up to
/// `last_accrual_ts`; yield earned since then is computed on demand from the
/// current time. `apr` is fixed-point (see [`crate::fixed_point::SCALE`]).
#[contracttype]
#[derive(Debug, Clone)]
pub struct YieldRecord {
    /// The staker who owns the position.
    pub staker: Address,
    /// The asset being staked (a symbol such as `XLM`, `USDC`).
    pub asset: Symbol,
    /// Principal currently staked, in the asset's base units.
    pub principal: i128,
    /// Current annual percentage rate for this position, fixed-point.
    pub apr: i128,
    /// Compounding model applied to this position.
    pub mode: CompoundingMode,
    /// Ledger timestamp (seconds) at which yield was last checkpointed.
    pub last_accrual_ts: u64,
    /// Yield realized and checkpointed up to `last_accrual_ts`, base units.
    pub accrued_yield: i128,
}

/// A single immutable entry in a staker/asset yield history log.
///
/// One entry is appended each time yield is checkpointed (accrued), the rate
/// changes, or yield is claimed, forming a complete, queryable audit trail.
#[contracttype]
#[derive(Debug, Clone)]
pub struct YieldHistoryEntry {
    /// Ledger timestamp (seconds) the entry covers up to.
    pub timestamp: u64,
    /// Duration in seconds this entry accounts for since the previous entry.
    pub period_seconds: u64,
    /// APR in effect over this period, fixed-point.
    pub apr: i128,
    /// Yield earned during this period, base units.
    pub yield_earned: i128,
    /// Cumulative unclaimed yield after this entry, base units.
    pub cumulative_yield: i128,
    /// True when this is a zero-period marker recording a yield claim.
    /// Claim markers have `yield_earned == 0` and `cumulative_yield == 0`.
    pub is_claim: bool,
}

/// A projected future-earnings estimate for a position.
#[contracttype]
#[derive(Debug, Clone)]
pub struct YieldProjection {
    /// Principal the projection is based on, base units.
    pub principal: i128,
    /// APR assumed for the projection, fixed-point.
    pub apr: i128,
    /// Compounding model assumed.
    pub mode: CompoundingMode,
    /// Horizon of the projection in seconds from now.
    pub horizon_seconds: u64,
    /// Projected yield over the horizon, base units.
    pub projected_yield: i128,
    /// Projected total balance (principal + yield) at the horizon, base units.
    pub projected_balance: i128,
    /// Effective APY implied by the assumed APR and mode, fixed-point.
    pub effective_apy: i128,
}

/// A scheduled yield distribution to a staker.
#[contracttype]
#[derive(Debug, Clone)]
pub struct DistributionSchedule {
    /// The staker to receive the distribution.
    pub staker: Address,
    /// The asset being distributed.
    pub asset: Symbol,
    /// Ledger timestamp (seconds) at which the distribution becomes due.
    pub due_ts: u64,
    /// Interval in seconds between recurring distributions (0 = one-off).
    pub interval_seconds: u64,
    /// Amount scheduled for distribution, base units.
    pub amount: i128,
    /// Whether this schedule has been fully distributed / closed.
    pub executed: bool,
}

// ---------------------------------------------------------------------------
// Multi-asset staking types
// ---------------------------------------------------------------------------

/// The unlock schedule variant controlling when a staked position can be
/// withdrawn.
///
/// - `Immediate`: No lock-up; the staker may withdraw at any time.
/// - `Cliff`: The full principal is locked until `unlock_ts`; after that the
///   entire position can be withdrawn.
/// - `Graduated`: The principal unlocks in equal tranches of `tranche_pct`
///   (basis points, 1 = 0.01%) every `interval_seconds` seconds, starting
///   at `start_ts`. Any remainder unlocks in the final tranche.
#[contracttype]
#[derive(Debug, Clone)]
pub enum UnlockSchedule {
    /// No lock-up.
    Immediate,
    /// Entire position unlocks at `unlock_ts` (ledger timestamp, seconds).
    Cliff(u64),
    /// Tranched unlock starting at `start_ts`, advancing every
    /// `interval_seconds` by `tranche_pct` basis points (1/10000).
    Graduated(GraduatedUnlock),
}

/// Parameters for a graduated (tranche-based) unlock schedule.
#[contracttype]
#[derive(Debug, Clone)]
pub struct GraduatedUnlock {
    /// Ledger timestamp (seconds) at which the first tranche unlocks.
    pub start_ts: u64,
    /// Seconds between consecutive tranches.
    pub interval_seconds: u64,
    /// Percentage of principal that unlocks per tranche, in basis points
    /// (1 bp = 0.01%).  For example, 1000 = 10% per tranche.
    pub tranche_pct_bps: u32,
}

/// Asset-specific yield configuration, independent per asset.
///
/// `min_stake` prevents dust positions; `max_stake` caps individual exposure.
/// Both values are in the asset's base units; `0` means no limit.
#[contracttype]
#[derive(Debug, Clone)]
pub struct AssetYieldRate {
    /// Asset this configuration applies to.
    pub asset: Symbol,
    /// Annual percentage rate for this asset, fixed-point.
    pub apr: i128,
    /// Compounding model for this asset.
    pub mode: CompoundingMode,
    /// Minimum stake required to open a position (0 = no minimum).
    pub min_stake: i128,
    /// Maximum stake allowed per staker (0 = no maximum).
    pub max_stake: i128,
    /// Unlock schedule applied to new positions for this asset.
    pub unlock_schedule: UnlockSchedule,
}

/// A staking position for a single `(staker, asset)` pair.
///
/// This is the authoritative source of truth for multi-asset staking. It
/// combines the staked principal with its asset-specific yield parameters,
/// lock-up state, and the snapshot of accrued yield so it can be queried
/// independently from the [`YieldRecord`] used by the lower-level engine.
#[contracttype]
#[derive(Debug, Clone)]
pub struct StakingPosition {
    /// Staker who owns this position.
    pub staker: Address,
    /// Asset staked in this position.
    pub asset: Symbol,
    /// Principal currently locked in this position, base units.
    pub principal: i128,
    /// APR in effect for this position, fixed-point.
    pub apr: i128,
    /// Compounding model for this position.
    pub mode: CompoundingMode,
    /// Ledger timestamp (seconds) at which the position was first opened.
    pub opened_at: u64,
    /// Unlock schedule for this position.
    pub unlock_schedule: UnlockSchedule,
    /// Cached accrued yield (updated on each checkpoint), base units.
    pub accrued_yield: i128,
}

/// A point-in-time snapshot of an entire portfolio's staking state.
///
/// Returned by `get_portfolio` and `portfolio_yield`; it aggregates across all
/// active positions for a staker.
#[contracttype]
#[derive(Debug, Clone)]
pub struct PortfolioSnapshot {
    /// Total principal staked across all assets, summed in their base units.
    /// (Heterogeneous assets are summed as raw `i128`; callers should apply
    /// USD or reference-price conversion for meaningful comparison.)
    pub total_principal: i128,
    /// Total accrued yield across all assets, base units (same caveat as above).
    pub total_accrued_yield: i128,
    /// Number of distinct assets held in the portfolio.
    pub asset_count: u32,
    /// Weighted-average APR across all positions (weighted by principal), fixed-point.
    pub weighted_avg_apr: i128,
    /// All active positions at the time of the snapshot.
    pub positions: Vec<StakingPosition>,
}

// ---------------------------------------------------------------------------
// Storage key enums
// ---------------------------------------------------------------------------

/// Storage keys for the yield engine's persistent data.
///
/// Keeping keys in a single enum avoids stringly-typed lookups and keeps the
/// storage layout easy to audit.
#[contracttype]
#[derive(Debug, Clone)]
pub enum YieldDataKey {
    /// The active [`YieldRecord`] for a `(staker, asset)` pair.
    Record(Address, Symbol),
    /// The [`YieldHistoryEntry`] list for a `(staker, asset)` pair.
    History(Address, Symbol),
    /// The [`DistributionSchedule`] list for a `(staker, asset)` pair.
    Schedule(Address, Symbol),
    /// The contract admin address set during `initialize`.
    Admin,
    /// The alert threshold value.
    AlertThreshold,
}

/// Default yield parameters applied when a position is first opened by a stake.
///
/// A single position, once opened, keeps its own APR and compounding mode across
/// subsequent stakes/unstakes (which only adjust principal); these defaults seed
/// brand-new positions and can be reconfigured before the first stake.
#[contracttype]
#[derive(Debug, Clone)]
pub struct StakingConfig {
    /// APR seeded onto a newly opened yield position, fixed-point (see
    /// [`crate::fixed_point::SCALE`]).
    pub default_apr: i128,
    /// Compounding mode seeded onto a newly opened yield position.
    pub default_mode: CompoundingMode,
}

/// Storage keys for the staking layer that sits in front of the yield engine.
#[contracttype]
#[derive(Debug, Clone)]
pub enum StakeDataKey {
    /// Staked balance (principal) for a `(staker, asset)` pair, base units.
    Balance(Address, Symbol),
    /// The default [`StakingConfig`] used when opening new positions.
    Config,
    /// Per-asset yield configuration [`AssetYieldRate`].
    AssetConfig(Symbol),
    /// The full [`StakingPosition`] for a `(staker, asset)` pair.
    Position(Address, Symbol),
    /// The list of asset [`Symbol`]s a staker currently has positions in.
    StakerAssets(Address),
}
