//! Soroban-typed records for portfolio valuation, snapshots, and performance
//! tracking.
//!
//! All types use `#[contracttype]` so they can be persisted in contract storage
//! and returned across the contract boundary. Values are represented using
//! fixed-point arithmetic (see [`crate::performance::SCALE`]) for deterministic,
//! cross-validator consistent results.

use soroban_sdk::{contracttype, Symbol};

// ---------------------------------------------------------------------------
// Fixed-point constants (mirrors the staking contract's conventions)
// ---------------------------------------------------------------------------

/// Scale factor: 1e18. A value of `1.0` is represented as `1_000_000_000_000_000_000`.
pub const SCALE: i128 = 1_000_000_000_000_000_000;

/// One whole unit in fixed-point representation.
pub const ONE: i128 = SCALE;

// ---------------------------------------------------------------------------
// Asset representation
// ---------------------------------------------------------------------------

/// A single asset held in a portfolio.
///
/// Tracks the asset's identity, quantity held, and its current price in
/// base currency units (fixed-point).
#[contracttype]
#[derive(Debug, Clone)]
pub struct PortfolioAsset {
    /// Asset symbol (e.g. `XLM`, `USDC`).
    pub asset: Symbol,
    /// Quantity of this asset held, in base units (integer).
    pub quantity: i128,
    /// Current price per unit in the portfolio's base currency, fixed-point.
    /// A price of 1.50 would be `1_500_000_000_000_000_000`.
    pub current_price: i128,
    /// Average cost basis per unit (what was paid on average), fixed-point.
    pub cost_basis: i128,
}

// ---------------------------------------------------------------------------
// Allocation
// ---------------------------------------------------------------------------

/// The allocation of a single asset expressed as a percentage of total portfolio
/// value. Percentage is in basis points (1 = 0.01%, 10000 = 100%).
#[contracttype]
#[derive(Debug, Clone)]
pub struct AssetAllocation {
    /// Asset symbol.
    pub asset: Symbol,
    /// Current market value (quantity × current_price) in fixed-point.
    pub market_value: i128,
    /// Allocation percentage in basis points (sum of all assets = 10_000 ± 10).
    pub allocation_bps: u32,
}

// ---------------------------------------------------------------------------
// Returns
// ---------------------------------------------------------------------------

/// Absolute and percentage returns for the portfolio.
#[contracttype]
#[derive(Debug, Clone)]
pub struct PortfolioReturns {
    /// Absolute return = current_value − initial_investment, fixed-point.
    pub absolute_return: i128,
    /// Percentage return = absolute_return / initial_investment, fixed-point.
    /// A value of 0.15 means +15%.
    pub percentage_return: i128,
}

// ---------------------------------------------------------------------------
// Performance metrics
// ---------------------------------------------------------------------------

/// Comprehensive performance metrics for a portfolio.
#[contracttype]
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    /// Annualized Sharpe ratio (excess return / volatility), fixed-point.
    /// A value of 1.50 means 1.50.
    pub sharpe_ratio: i128,
    /// Annualized Sortino ratio (excess return / downside deviation), fixed-point.
    pub sortino_ratio: i128,
    /// Maximum drawdown from peak to trough, expressed as a positive fraction
    /// (e.g. 0.25 means a 25% drawdown), fixed-point.
    pub max_drawdown: i128,
    /// Time-weighted return over the evaluation period, fixed-point.
    /// A value of 0.10 means +10%.
    pub time_weighted_return: i128,
}

// ---------------------------------------------------------------------------
// Snapshots & history
// ---------------------------------------------------------------------------

/// A point-in-time snapshot of the entire portfolio, used for historical
/// comparison and drawdown computation.
#[contracttype]
#[derive(Debug, Clone)]
pub struct PortfolioSnapshot {
    /// Ledger timestamp (seconds) when this snapshot was taken.
    pub timestamp: u64,
    /// Total portfolio value at this moment, fixed-point.
    pub total_value: i128,
    /// Total initial investment (cumulative cost basis), fixed-point.
    pub total_cost_basis: i128,
    /// Number of assets in the portfolio at this moment.
    pub asset_count: u32,
}

/// A single entry in the valuation history log.
///
/// Appended each time a snapshot is recorded, providing an ordered audit trail
/// for trend analysis and return computation.
#[contracttype]
#[derive(Debug, Clone)]
pub struct ValuationHistoryEntry {
    /// Ledger timestamp (seconds).
    pub timestamp: u64,
    /// Total portfolio value at this timestamp, fixed-point.
    pub total_value: i128,
    /// Absolute return at this point, fixed-point.
    pub absolute_return: i128,
    /// Percentage return at this point, fixed-point.
    pub percentage_return: i128,
    /// Per-asset market values at this point.
    pub asset_values: soroban_sdk::Map<Symbol, i128>,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

/// Storage keys for the valuation contract's persistent data.
#[contracttype]
#[derive(Debug, Clone)]
pub enum ValuationDataKey {
    /// The list of [`PortfolioAsset`] entries for a portfolio.
    Assets(Symbol),
    /// Initial investment for a portfolio (fixed-point).
    InitialInvestment(Symbol),
    /// Snapshot history for a portfolio.
    Snapshots(Symbol),
    /// Valuation history for a portfolio.
    ValuationHistory(Symbol),
    /// The latest [`PortfolioSnapshot`] for a portfolio.
    LatestSnapshot(Symbol),
    /// Risk-free rate used in Sharpe/Sortino calculations (fixed-point, annualized).
    RiskFreeRate,
    /// Number of periods (in seconds) used for annualization in metrics.
    AnnualizationPeriod,
    /// Daily returns history for a portfolio (Vec of fixed-point returns).
    DailyReturns(Symbol),
}
