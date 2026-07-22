//! Soroban-typed records for tracking yield accrual, history, and schedules.
//!
//! These types are marked `#[contracttype]` so they can be persisted in
//! contract storage and returned across the contract boundary. They wrap the
//! pure-math results from [`crate::compounding`] and [`crate::apy`] into durable,
//! queryable structures keyed by staker and asset.

use soroban_sdk::{contracttype, Address, Symbol};

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
}
