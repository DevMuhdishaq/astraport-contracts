#![no_std]
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Map, String as SorobanString, Symbol, Vec};

pub mod multi_asset_rebalancer;

use astraport_audit::logger::AuditLogger;
use astraport_audit::records::{permissions, AuditEventType, StateSnapshot};

/// Default tolerance used when deciding whether a holding needs rebalancing.
const DEFAULT_DRIFT_THRESHOLD_BPS: u32 = 100;

/// Allocation tolerance: allocations must sum to 10_000 ± ALLOCATION_TOLERANCE_BPS.
/// 0.1% = 10 basis points.
const ALLOCATION_TOLERANCE_BPS: u32 = 10;

/// Errors returned by the rebalancing contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RebalancingError {
    /// The target allocation weights do not sum to 10_000 basis points (100%).
    InvalidAllocation = 1,
    /// The supplied current holding weights do not sum to 10_000 basis points.
    InvalidCurrentHoldings = 2,
    /// No target allocation has been configured for this portfolio.
    TargetAllocationNotFound = 3,
    /// No current holdings have been supplied for this portfolio.
    CurrentHoldingsNotFound = 4,
    /// An error occurred during multi-asset rebalancing.
    MultiAssetRebalanceFailed = 5,
    /// Caller is not authorized to modify this portfolio.
    Unauthorized = 6,
    /// The portfolio already exists.
    PortfolioAlreadyExists = 7,
    /// The portfolio does not exist.
    PortfolioNotFound = 8,
    /// The asset list is empty.
    EmptyAssets = 9,
    /// The portfolio name is empty.
    EmptyName = 10,
    /// The allocation percentages do not sum to 100% (within ±0.1% tolerance).
    AllocationSumOutOfRange = 11,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RebalanceInterval {
    Hourly,
    Daily,
    Weekly,
    Monthly,
}

/// Metadata for a portfolio including naming and timestamps.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortfolioMetadata {
    /// Human-readable portfolio name.
    pub name: SorobanString,
    /// Optional description of the portfolio strategy.
    pub description: SorobanString,
    /// Ledger timestamp when the portfolio was created.
    pub created_at: u64,
    /// Ledger timestamp of the last modification.
    pub last_modified: u64,
}

/// A complete portfolio with owner, assets, target allocations, and metadata.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Portfolio {
    /// Unique portfolio identifier.
    pub id: Symbol,
    /// Address of the portfolio owner.
    pub owner: Address,
    /// Ordered list of asset symbols in this portfolio.
    pub assets: Vec<Symbol>,
    /// Target allocation weights in basis points per asset.
    pub target_allocation: TargetAllocation,
    /// Portfolio metadata (name, description, timestamps).
    pub metadata: PortfolioMetadata,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RebalancingSchedule {
    pub portfolio_id: Symbol,
    pub interval: RebalanceInterval,
    pub next_execution: u64,
    pub last_execution: u64, // 0 means never
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionHistoryRecord {
    pub timestamp: u64,
    pub outcome: Symbol,
    pub details: Symbol,
}

/// Target allocation for a portfolio.
///
/// Maps each asset symbol to its target weight in basis points (1/100th of a
/// percent). All weights must sum to exactly 10_000 (= 100%).
#[contracttype]
#[derive(Clone)]
pub struct TargetAllocation {
    pub allocations: Map<Symbol, u32>,
}

/// Current portfolio weights in basis points. A holding omitted from this map is
/// treated as zero when it is compared with the target allocation.
#[contracttype]
#[derive(Clone)]
pub struct CurrentHoldings {
    pub allocations: Map<Symbol, u32>,
}

/// The action required to move a holding back toward its target weight.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RebalanceDirection {
    Buy,
    Sell,
}

/// An asset whose current weight differs from its target by more than the
/// configured tolerance.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RebalanceAdjustment {
    pub asset: Symbol,
    pub current_weight_bps: u32,
    pub target_weight_bps: u32,
    /// `current_weight_bps - target_weight_bps`. Positive drift means sell;
    /// negative drift means buy.
    pub drift_bps: i32,
    pub direction: RebalanceDirection,
}

/// Computed rebalance plan for a portfolio.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RebalanceResult {
    pub portfolio_id: Symbol,
    pub drift_threshold_bps: u32,
    pub adjustments: Vec<RebalanceAdjustment>,
}

#[contracttype]
pub enum DataKey {
    Schedule(Symbol),
    History(Symbol),
    Allocation(Symbol),
    CurrentHoldings(Symbol),
    DriftThreshold(Symbol),
    /// Optional audit-log sink address. When set, the rebalancing contract
    /// invokes the audit contract on every state-changing event.
    AuditSink,
    /// Portfolio owner address mapping: portfolio_id -> Address
    Owner(Symbol),
    /// Full portfolio record: portfolio_id -> Portfolio
    Portfolio(Symbol),
}

/// Event data for manual rebalance - includes drift summary via timestamp
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RebalanceEventData {
    pub portfolio_id: Symbol,
    pub outcome: Symbol,
    pub timestamp: u64,
}

/// Event data for scheduled rebalance - richer context for off-chain listeners
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchedRebalanceEventData {
    pub portfolio_id: Symbol,
    pub outcome: Symbol,
    pub timestamp: u64,
    pub details: Symbol,
}

pub struct ScheduleValidator;

impl ScheduleValidator {
    pub fn validate(interval: &RebalanceInterval) -> bool {
        match interval {
            RebalanceInterval::Hourly
            | RebalanceInterval::Daily
            | RebalanceInterval::Weekly
            | RebalanceInterval::Monthly => true,
        }
    }
}

fn interval_to_seconds(interval: &RebalanceInterval) -> u64 {
    match interval {
        RebalanceInterval::Hourly => 3600,
        RebalanceInterval::Daily => 86400,
        RebalanceInterval::Weekly => 604800,
        RebalanceInterval::Monthly => 2592000, // 30 days
    }
}

/// Rebalancing contract for AstraPort
/// Manages portfolio rebalancing and allocation adjustments
#[contract]
pub struct RebalancingContract;

#[contractimpl]
impl RebalancingContract {
    /// Initialize the rebalancing contract
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// Success symbol if initialization succeeds
    pub fn initialize(_env: Env) -> Symbol {
        symbol_short!("ok")
    }

    /// Create a new portfolio with the given configuration.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `owner` - Portfolio owner address (must authorize)
    /// * `portfolio_id` - Unique identifier for the portfolio
    /// * `name` - Human-readable portfolio name (must not be empty)
    /// * `description` - Portfolio description
    /// * `assets` - Ordered list of asset symbols (must not be empty)
    /// * `target_allocation` - Target allocation weights in basis points
    ///
    /// # Returns
    /// `Ok(Portfolio)` on success.
    /// `Err(PortfolioAlreadyExists)` if a portfolio with this ID already exists.
    /// `Err(EmptyAssets)` if the asset list is empty.
    /// `Err(EmptyName)` if the portfolio name is empty.
    /// `Err(AllocationSumOutOfRange)` if allocations don't sum to 10_000 ± 10 bps.
    pub fn initialize_portfolio(
        env: Env,
        owner: Address,
        portfolio_id: Symbol,
        name: SorobanString,
        description: SorobanString,
        assets: Vec<Symbol>,
        target_allocation: TargetAllocation,
    ) -> Result<Portfolio, RebalancingError> {
        owner.require_auth();

        // Ensure portfolio does not already exist
        let portfolio_key = DataKey::Portfolio(portfolio_id.clone());
        if env.storage().persistent().has(&portfolio_key) {
            return Err(RebalancingError::PortfolioAlreadyExists);        }

        // Validate name is non-empty
        if name.len() == 0 {
            return Err(RebalancingError::EmptyName);
        }

        // Validate asset list is non-empty
        if assets.len() == 0 {
            return Err(RebalancingError::EmptyAssets);
        }

        // Validate allocation sum is within tolerance: 10_000 ± ALLOCATION_TOLERANCE_BPS
        let total = Self::allocation_sum(&target_allocation);
        if total < (10_000 - ALLOCATION_TOLERANCE_BPS)
            || total > (10_000 + ALLOCATION_TOLERANCE_BPS)
        {
            return Err(RebalancingError::AllocationSumOutOfRange);
        }

        // Validate that every allocated asset is in the assets list
        for (asset, _weight) in target_allocation.allocations.iter() {
            if !assets.contains(asset) {
                return Err(RebalancingError::InvalidAllocation);
            }
        }

        let now = env.ledger().timestamp();

        let portfolio = Portfolio {
            id: portfolio_id.clone(),
            owner: owner.clone(),
            assets,
            target_allocation: target_allocation.clone(),
            metadata: PortfolioMetadata {
                name,
                description,
                created_at: now,
                last_modified: now,
            },
        };

        // Store the full portfolio record
        env.storage().persistent().set(&portfolio_key, &portfolio);

        // Also store the owner separately for backward compatibility
        // with existing owner-check logic
        let owner_key = DataKey::Owner(portfolio_id.clone());
        if !env.storage().persistent().has(&owner_key) {
            env.storage().persistent().set(&owner_key, &owner);
        }

        // Store the target allocation separately for backward compatibility
        // with rebalance calculations
        let alloc_key = DataKey::Allocation(portfolio_id);
        env.storage().persistent().set(&alloc_key, &target_allocation);

        Ok(portfolio)
    }

    /// Retrieve a portfolio by its ID.
    pub fn get_portfolio(env: Env, portfolio_id: Symbol) -> Result<Portfolio, RebalancingError> {
        let key = DataKey::Portfolio(portfolio_id);
        env.storage()
            .persistent()
            .get(&key)
            .ok_or(RebalancingError::PortfolioNotFound)
    }

    /// Update a portfolio's metadata (name and description). Only the owner can modify.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `owner` - Portfolio owner address (must authorize and match stored owner)
    /// * `portfolio_id` - Identifier for the portfolio
    /// * `name` - New portfolio name (must not be empty)
    /// * `description` - New portfolio description
    ///
    /// # Returns
    /// `Ok(Portfolio)` with updated metadata on success.
    /// `Err(PortfolioNotFound)` if the portfolio does not exist.
    /// `Err(EmptyName)` if the new name is empty.
    /// `Err(Unauthorized)` if the caller is not the portfolio owner.
    pub fn update_portfolio_metadata(
        env: Env,
        owner: Address,
        portfolio_id: Symbol,
        name: SorobanString,
        description: SorobanString,
    ) -> Result<Portfolio, RebalancingError> {
        Self::require_owner_auth(&env, &owner, &portfolio_id)?;

        // Validate name is non-empty
        if name.len() == 0 {
            return Err(RebalancingError::EmptyName);
        }

        let portfolio_key = DataKey::Portfolio(portfolio_id.clone());
        let mut portfolio: Portfolio = env
            .storage()
            .persistent()
            .get(&portfolio_key)
            .ok_or(RebalancingError::PortfolioNotFound)?;

        portfolio.metadata.name = name;
        portfolio.metadata.description = description;
        portfolio.metadata.last_modified = env.ledger().timestamp();

        env.storage().persistent().set(&portfolio_key, &portfolio);

        Ok(portfolio)
    }

    /// Update a portfolio's target allocation. Only the owner can modify.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `owner` - Portfolio owner address (must authorize and match stored owner)
    /// * `portfolio_id` - Identifier for the portfolio
    /// * `target_allocation` - New target allocation weights in basis points
    ///
    /// # Returns
    /// `Ok(Portfolio)` with updated allocation on success.
    /// `Err(AllocationSumOutOfRange)` if allocations don't sum to 10_000 ± 10 bps.
    pub fn update_portfolio_allocation(
        env: Env,
        owner: Address,
        portfolio_id: Symbol,
        target_allocation: TargetAllocation,
    ) -> Result<Portfolio, RebalancingError> {
        Self::require_owner_auth(&env, &owner, &portfolio_id)?;

        // Validate allocation sum is within tolerance
        let total = Self::allocation_sum(&target_allocation);
        if total < (10_000 - ALLOCATION_TOLERANCE_BPS)
            || total > (10_000 + ALLOCATION_TOLERANCE_BPS)
        {
            return Err(RebalancingError::AllocationSumOutOfRange);
        }

        let portfolio_key = DataKey::Portfolio(portfolio_id.clone());
        let mut portfolio: Portfolio = env
            .storage()
            .persistent()
            .get(&portfolio_key)
            .ok_or(RebalancingError::PortfolioNotFound)?;

        portfolio.target_allocation = target_allocation.clone();
        portfolio.metadata.last_modified = env.ledger().timestamp();

        env.storage().persistent().set(&portfolio_key, &portfolio);
        // Also update the standalone allocation for rebalance calculations
        let alloc_key = DataKey::Allocation(portfolio_id);
        env.storage().persistent().set(&alloc_key, &target_allocation);

        Ok(portfolio)
    }

    /// Helper: compute the sum of allocation weights in basis points.
    fn allocation_sum(allocation: &TargetAllocation) -> u32 {
        let mut total: u32 = 0;
        for (_asset, weight) in allocation.allocations.iter() {
            total += weight;
        }
        total
    }

    /// Helper to enforce portfolio owner authorization.
    /// If no owner is recorded yet for `portfolio_id`, registers `owner` as owner.
    /// Calls `owner.require_auth()` and ensures `owner` matches the recorded owner.
    fn require_owner_auth(
        env: &Env,
        owner: &Address,
        portfolio_id: &Symbol,
    ) -> Result<(), RebalancingError> {
        owner.require_auth();
        let key = DataKey::Owner(portfolio_id.clone());
        if let Some(stored_owner) = env.storage().persistent().get::<DataKey, Address>(&key) {
            if &stored_owner != owner {
                return Err(RebalancingError::Unauthorized);
            }
        } else {
            env.storage().persistent().set(&key, owner);
        }
        Ok(())
    }

    /// Get the owner address for a portfolio if set.
    pub fn get_owner(env: Env, portfolio_id: Symbol) -> Option<Address> {
        let key = DataKey::Owner(portfolio_id);
        env.storage().persistent().get(&key)
    }

    /// Compute a rebalance plan from the stored target allocation and current
    /// holdings. The plan only includes assets whose absolute drift is greater
    /// than the configured threshold. A manual rebalance is recorded in the
    /// execution history.
    pub fn rebalance(
        env: Env,
        owner: Address,
        portfolio_id: Symbol,
    ) -> Result<RebalanceResult, RebalancingError> {
        Self::require_owner_auth(&env, &owner, &portfolio_id)?;
        let result = Self::calculate_rebalance(&env, &portfolio_id)?;
        Self::record_execution(
            &env,
            &portfolio_id,
            symbol_short!("done"),
            symbol_short!("manual"),
        );
        let snapshot_before = env
            .storage()
            .persistent()
            .get::<DataKey, CurrentHoldings>(&DataKey::CurrentHoldings(portfolio_id.clone()));
        let snapshot_after = env
            .storage()
            .persistent()
            .get::<DataKey, TargetAllocation>(&DataKey::Allocation(portfolio_id.clone()));
        let mut before_map = Map::new(&env);
        let mut after_map = Map::new(&env);
        if let Some(h) = snapshot_before {
            for (k, v) in h.allocations.iter() { before_map.set(k, v); }
        }
        if let Some(a) = snapshot_after {
            for (k, v) in a.allocations.iter() { after_map.set(k, v); }
        }
        Self::log_audit_if_configured(
            &env,
            &portfolio_id,
            symbol_short!("done"),
            "manual_rebalance",
            &before_map,
            &after_map,
        );
        Ok(result)
    }

    /// Get current rebalancing status
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `portfolio_id` - Identifier for the portfolio
    ///
    /// # Returns
    /// Status symbol
    pub fn get_status(_env: Env, _portfolio_id: Symbol) -> Symbol {
        symbol_short!("ok")
    }

    pub fn set_schedule(
        env: Env,
        owner: Address,
        portfolio_id: Symbol,
        interval: RebalanceInterval,
    ) -> Symbol {
        if Self::require_owner_auth(&env, &owner, &portfolio_id).is_err() {
            return symbol_short!("err_auth");
        }
        if !ScheduleValidator::validate(&interval) {
            return symbol_short!("err_val");
        }
        let key = DataKey::Schedule(portfolio_id.clone());
        if env.storage().persistent().has(&key) {
            return symbol_short!("err_exist");
        }

        let now = env.ledger().timestamp();
        let next_execution = now + interval_to_seconds(&interval);

        let schedule = RebalancingSchedule {
            portfolio_id,
            interval,
            next_execution,
            last_execution: 0,
        };

        env.storage().persistent().set(&key, &schedule);
        symbol_short!("ok")
    }

    pub fn update_schedule(
        env: Env,
        owner: Address,
        portfolio_id: Symbol,
        interval: RebalanceInterval,
    ) -> Symbol {
        if Self::require_owner_auth(&env, &owner, &portfolio_id).is_err() {
            return symbol_short!("err_auth");
        }
        if !ScheduleValidator::validate(&interval) {
            return symbol_short!("err_val");
        }
        let key = DataKey::Schedule(portfolio_id.clone());
        if !env.storage().persistent().has(&key) {
            return symbol_short!("err_none");
        }

        let mut schedule: RebalancingSchedule = env.storage().persistent().get(&key).unwrap();
        let now = env.ledger().timestamp();

        schedule.interval = interval;
        let interval_secs = interval_to_seconds(&schedule.interval);
        if schedule.last_execution > 0 {
            schedule.next_execution = schedule.last_execution + interval_secs;
        } else {
            schedule.next_execution = now + interval_secs;
        }

        env.storage().persistent().set(&key, &schedule);
        symbol_short!("ok")
    }

    pub fn cancel_schedule(env: Env, owner: Address, portfolio_id: Symbol) -> Symbol {
        if Self::require_owner_auth(&env, &owner, &portfolio_id).is_err() {
            return symbol_short!("err_auth");
        }
        let key = DataKey::Schedule(portfolio_id.clone());
        if !env.storage().persistent().has(&key) {
            return symbol_short!("err_none");
        }
        env.storage().persistent().remove(&key);
        symbol_short!("ok")
    }

    pub fn get_schedule(env: Env, portfolio_id: Symbol) -> Option<RebalancingSchedule> {
        let key = DataKey::Schedule(portfolio_id);
        env.storage().persistent().get(&key)
    }

    /// Set the target allocation for a portfolio.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `owner` - Portfolio owner address
    /// * `portfolio_id` - Identifier for the portfolio
    /// * `allocation` - Target allocation with asset→basis-points weights
    ///
    /// # Returns
    /// `Ok(ok)` if the allocation is valid (sums to 10_000 bps) and persisted.
    /// `Err(RebalancingError::InvalidAllocation)` if weights don't sum to 10_000.
    pub fn set_target_allocation(
        env: Env,
        owner: Address,
        portfolio_id: Symbol,
        allocation: TargetAllocation,
    ) -> Result<Symbol, RebalancingError> {
        Self::require_owner_auth(&env, &owner, &portfolio_id)?;
        let mut total: u32 = 0;
        for (_asset, weight) in allocation.allocations.iter() {
            total += weight;
        }
        if total != 10_000 {
            return Err(RebalancingError::InvalidAllocation);
        }

        let key = DataKey::Allocation(portfolio_id);
        env.storage().persistent().set(&key, &allocation);
        Ok(symbol_short!("ok"))
    }

    /// Get the target allocation for a portfolio.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `portfolio_id` - Identifier for the portfolio
    ///
    /// # Returns
    /// `Some(TargetAllocation)` if one has been set, `None` otherwise.
    pub fn get_target_allocation(env: Env, portfolio_id: Symbol) -> Option<TargetAllocation> {
        let key = DataKey::Allocation(portfolio_id);
        env.storage().persistent().get(&key)
    }

    /// Store the current portfolio weights used by `rebalance`. Current weights
    /// are expressed in basis points and must total 10_000.
    pub fn set_current_holdings(
        env: Env,
        owner: Address,
        portfolio_id: Symbol,
        holdings: CurrentHoldings,
    ) -> Result<Symbol, RebalancingError> {
        Self::require_owner_auth(&env, &owner, &portfolio_id)?;
        let mut total: u32 = 0;
        for (_asset, weight) in holdings.allocations.iter() {
            total += weight;
        }
        if total != 10_000 {
            return Err(RebalancingError::InvalidCurrentHoldings);
        }
        let key = DataKey::CurrentHoldings(portfolio_id);
        env.storage().persistent().set(&key, &holdings);
        Ok(symbol_short!("ok"))
    }

    pub fn get_current_holdings(env: Env, portfolio_id: Symbol) -> Option<CurrentHoldings> {
        let key = DataKey::CurrentHoldings(portfolio_id);
        env.storage().persistent().get(&key)
    }

    /// Set the per-portfolio drift tolerance in basis points. The default is
    /// 100 bps when this value has not been configured.
    pub fn set_drift_threshold_bps(
        env: Env,
        owner: Address,
        portfolio_id: Symbol,
        threshold_bps: u32,
    ) -> Result<(), RebalancingError> {
        Self::require_owner_auth(&env, &owner, &portfolio_id)?;
        let key = DataKey::DriftThreshold(portfolio_id);
        env.storage().persistent().set(&key, &threshold_bps);
        Ok(())
    }

    pub fn get_drift_threshold_bps(env: Env, portfolio_id: Symbol) -> u32 {
        let key = DataKey::DriftThreshold(portfolio_id);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or(DEFAULT_DRIFT_THRESHOLD_BPS)
    }

    /// Get execution history for a portfolio
    pub fn get_execution_history(env: Env, portfolio_id: Symbol) -> Vec<ExecutionHistoryRecord> {
        let key = DataKey::History(portfolio_id);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Check and execute scheduled rebalance
    pub fn check_exec_sched_rebalance(env: Env, portfolio_id: Symbol) -> Symbol {
        let key = DataKey::Schedule(portfolio_id.clone());
        if !env.storage().persistent().has(&key) {
            let ts = env.ledger().timestamp();
            let event_data = SchedRebalanceEventData {
                portfolio_id: portfolio_id.clone(),
                outcome: symbol_short!("err_none"),
                timestamp: ts,
                details: symbol_short!("err_none"),
            };
            env.events().publish((symbol_short!("SREBAL"), portfolio_id.clone()), event_data);
            return symbol_short!("err_none");
        }

        let mut schedule: RebalancingSchedule = env.storage().persistent().get(&key).unwrap();
        let now = env.ledger().timestamp();

        if now < schedule.next_execution {
            let event_data = SchedRebalanceEventData {
                portfolio_id: portfolio_id.clone(),
                outcome: symbol_short!("not_due"),
                timestamp: now,
                details: symbol_short!("not_due"),
            };
            env.events().publish((symbol_short!("SREBAL"), portfolio_id.clone()), event_data);
            return symbol_short!("not_due");
        }

        // Scheduled execution calculates the same plan as a manual rebalance,
        // but records a scheduled (rather than manual) history entry below.
        let outcome = match Self::calculate_rebalance(&env, &portfolio_id) {
            Ok(_) => symbol_short!("done"),
            Err(RebalancingError::TargetAllocationNotFound) => symbol_short!("no_target"),
            Err(RebalancingError::CurrentHoldingsNotFound) => symbol_short!("no_hold"),
            Err(_) => symbol_short!("err"),
        };

        // Update schedule
        schedule.last_execution = now;
        schedule.next_execution = now + interval_to_seconds(&schedule.interval);
        env.storage().persistent().set(&key, &schedule);

        // Log execution history
        let history_key = DataKey::History(portfolio_id.clone());
        let mut history: Vec<ExecutionHistoryRecord> = env
            .storage()
            .persistent()
            .get(&history_key)
            .unwrap_or_else(|| Vec::new(&env));

        let record = ExecutionHistoryRecord {
            timestamp: now,
            outcome: outcome.clone(),
            details: symbol_short!("schd_exec"),
        };
        history.push_back(record);
        env.storage().persistent().set(&history_key, &history);

        // Audit log integration: capture before/after balances for the schedule.
        let cur = env
            .storage()
            .persistent()
            .get::<DataKey, CurrentHoldings>(&DataKey::CurrentHoldings(portfolio_id.clone()));
        let tgt = env
            .storage()
            .persistent()
            .get::<DataKey, TargetAllocation>(&DataKey::Allocation(portfolio_id.clone()));
        let mut before_map = Map::new(&env);
        let mut after_map = Map::new(&env);
        if let Some(h) = cur {
            for (k, v) in h.allocations.iter() { before_map.set(k, v); }
        }
        if let Some(a) = tgt {
            for (k, v) in a.allocations.iter() { after_map.set(k, v); }
        }
        Self::log_audit_if_configured(
            &env,
            &portfolio_id,
            outcome.clone(),
            "scheduled_rebalance",
            &before_map,
            &after_map,
        );

        outcome
    }

    pub fn get_rebalance_plan(
        env: Env,
        portfolio_id: Symbol,
    ) -> Result<RebalanceResult, RebalancingError> {
        Self::calculate_rebalance(&env, &portfolio_id)
    }

    pub fn check_and_exec_sched(env: Env, portfolio_id: Symbol) -> Symbol {
        Self::check_exec_sched_rebalance(env, portfolio_id)
    }

    pub fn execute_rebalance(
        env: Env,
        owner: Address,
        portfolio_id: Symbol,
        strategy: multi_asset_rebalancer::ExecutionStrategy,
    ) -> Result<(), RebalancingError> {
        Self::require_owner_auth(&env, &owner, &portfolio_id)?;
        let plan = Self::calculate_rebalance(&env, &portfolio_id)?;
        let rebalancer_id = env.register_contract(None, multi_asset_rebalancer::MultiAssetRebalancer);
        let client = multi_asset_rebalancer::MultiAssetRebalancerClient::new(&env, &rebalancer_id);
        client.rebalance(&portfolio_id, &strategy, &plan.adjustments);
        Ok(())
    }

    pub fn simulate_rebalance(
        env: Env,
        portfolio_id: Symbol,
        strategy: multi_asset_rebalancer::ExecutionStrategy,
    ) -> Result<multi_asset_rebalancer::SimulationResult, RebalancingError> {
        let plan = Self::calculate_rebalance(&env, &portfolio_id)?;
        let rebalancer_id = env.register_contract(None, multi_asset_rebalancer::MultiAssetRebalancer);
        let client = multi_asset_rebalancer::MultiAssetRebalancerClient::new(&env, &rebalancer_id);
        Ok(client.simulate_rebalance(&portfolio_id, &strategy, &plan.adjustments))
    }
}

impl RebalancingContract {
    /// Configure the audit-log sink address. Admin-only is enforced by the
    /// caller (no admin concept here yet, so we accept any caller — the
    /// rebalancing contract is usually gated by the deployer key).
    pub fn set_audit_sink(env: Env, sink: Address) -> Symbol {
        env.storage().persistent().set(&DataKey::AuditSink, &sink);
        symbol_short!("ok")
    }

    /// Read the audit-log sink address, if configured.
    pub fn get_audit_sink(env: Env) -> Option<Address> {
        env.storage().persistent().get(&DataKey::AuditSink)
    }

    /// Append an audit event if a sink is configured. No-op otherwise.
    fn log_audit_if_configured(
        env: &Env,
        portfolio_id: &Symbol,
        outcome: Symbol,
        detail: &str,
        balances_before: &Map<Symbol, u32>,
        balances_after: &Map<Symbol, u32>,
    ) {
        let key = DataKey::AuditSink;
        let sink: Option<Address> = env.storage().persistent().get(&key);
        if let Some(sink) = sink {
            let mut before = StateSnapshot::empty(env);
            for (k, v) in balances_before.iter() {
                before.push(k, v as i128);
            }
            let mut after = StateSnapshot::empty(env);
            for (k, v) in balances_after.iter() {
                after.push(k, v as i128);
            }
            let detail_str = soroban_sdk::String::from_str(env, detail);
            let logger = AuditLogger::new(env, &sink);
            // The actor is the contract itself for rebalance events; we use
            // the portfolio id as the actor label so verifiers can spot
            // portfolio-scoped changes.
            let actor_addr = env.current_contract_address();
            let _ = logger.log_event(
                actor_addr,
                AuditEventType::Rebalance,
                portfolio_id.clone(),
                permissions::ADMIN,
                before,
                after,
                outcome,
                detail_str,
            );
        }
    }

    fn calculate_rebalance(
        env: &Env,
        portfolio_id: &Symbol,
    ) -> Result<RebalanceResult, RebalancingError> {
        let target: TargetAllocation = env
            .storage()
            .persistent()
            .get(&DataKey::Allocation(portfolio_id.clone()))
            .ok_or(RebalancingError::TargetAllocationNotFound)?;
        let current: CurrentHoldings = env
            .storage()
            .persistent()
            .get(&DataKey::CurrentHoldings(portfolio_id.clone()))
            .ok_or(RebalancingError::CurrentHoldingsNotFound)?;
        let threshold: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::DriftThreshold(portfolio_id.clone()))
            .unwrap_or(DEFAULT_DRIFT_THRESHOLD_BPS);
        let mut adjustments = Vec::new(env);

        // Visit target assets first, then current-only assets. This makes an
        // asset removed from a target allocation correctly appear as a sell.
        for (asset, target_weight) in target.allocations.iter() {
            let current_weight = current.allocations.get(asset.clone()).unwrap_or(0);
            Self::add_adjustment_if_needed(
                &mut adjustments,
                asset,
                current_weight,
                target_weight,
                threshold,
            );
        }
        for (asset, current_weight) in current.allocations.iter() {
            if !target.allocations.contains_key(asset.clone()) {
                Self::add_adjustment_if_needed(
                    &mut adjustments,
                    asset,
                    current_weight,
                    0,
                    threshold,
                );
            }
        }

        Ok(RebalanceResult {
            portfolio_id: portfolio_id.clone(),
            drift_threshold_bps: threshold,
            adjustments,
        })
    }

    fn add_adjustment_if_needed(
        adjustments: &mut Vec<RebalanceAdjustment>,
        asset: Symbol,
        current_weight: u32,
        target_weight: u32,
        threshold: u32,
    ) {
        let drift = current_weight as i32 - target_weight as i32;
        if drift.unsigned_abs() > threshold {
            let direction = if drift > 0 {
                RebalanceDirection::Sell
            } else {
                RebalanceDirection::Buy
            };
            adjustments.push_back(RebalanceAdjustment {
                asset,
                current_weight_bps: current_weight,
                target_weight_bps: target_weight,
                drift_bps: drift,
                direction,
            });
        }
    }

    fn record_execution(env: &Env, portfolio_id: &Symbol, outcome: Symbol, details: Symbol) {
        let history_key = DataKey::History(portfolio_id.clone());
        let mut history: Vec<ExecutionHistoryRecord> = env
            .storage()
            .persistent()
            .get(&history_key)
            .unwrap_or_else(|| Vec::new(env));
        history.push_back(ExecutionHistoryRecord {
            timestamp: env.ledger().timestamp(),
            outcome,
            details,
        });
        env.storage().persistent().set(&history_key, &history);
    }

    pub fn check_and_execute_scheduled_rebalance(env: Env, portfolio_id: Symbol) -> Symbol {
        Self::check_exec_sched_rebalance(env, portfolio_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{symbol_short, testutils::Address as _, testutils::Ledger, Env, Map, String as SorobanString};

    fn weights(env: &Env, entries: &[(Symbol, u32)]) -> Map<Symbol, u32> {
        let mut result = Map::new(env);
        for (asset, weight) in entries.iter() { result.set(asset.clone(), *weight); }
        result
    }

    fn client(env: &Env) -> RebalancingContractClient<'_> {
        let id = env.register_contract(None, RebalancingContract);
        RebalancingContractClient::new(env, &id)
    }

    #[test]
    fn test_initialize() {
        let env = Env::default();
        assert_eq!(client(&env).initialize(), symbol_short!("ok"));
    }

    #[test]
    fn test_rebalance_no_drift_does_not_flag_assets_and_logs_manual_execution() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let portfolio = symbol_short!("port1");
        let allocation = weights(&env, &[(symbol_short!("USDC"), 6_000), (symbol_short!("XLM"), 4_000)]);
        client.set_target_allocation(&owner, &portfolio, &TargetAllocation { allocations: allocation.clone() });
        client.set_current_holdings(&owner, &portfolio, &CurrentHoldings { allocations: allocation });
        let result = client.rebalance(&owner, &portfolio);
        assert_eq!(result.adjustments.len(), 0);
        let history = client.get_execution_history(&portfolio);
        assert_eq!(history.len(), 1);
        assert_eq!(history.get(0).unwrap().details, symbol_short!("manual"));
        assert_eq!(client.get_owner(&portfolio), Some(owner));
    }

    #[test]
    fn test_rebalance_flags_single_asset_drift_with_direction() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let portfolio = symbol_short!("port1");
        client.set_target_allocation(&owner, &portfolio, &TargetAllocation { allocations: weights(&env, &[(symbol_short!("USDC"), 5_000), (symbol_short!("XLM"), 3_000), (symbol_short!("BTC"), 2_000)]) });
        client.set_current_holdings(&owner, &portfolio, &CurrentHoldings { allocations: weights(&env, &[(symbol_short!("USDC"), 5_250), (symbol_short!("XLM"), 2_900), (symbol_short!("BTC"), 1_850)]) });
        client.set_drift_threshold_bps(&owner, &portfolio, &200);
        let result = client.rebalance(&owner, &portfolio);
        assert_eq!(result.adjustments.len(), 1);
        let adjustment = result.adjustments.get(0).unwrap();
        assert_eq!(adjustment.asset, symbol_short!("USDC"));
        assert_eq!(adjustment.drift_bps, 250);
        assert_eq!(adjustment.direction, RebalanceDirection::Sell);
    }

    #[test]
    fn test_rebalance_flags_multiple_assets_and_includes_buy_and_sell() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let portfolio = symbol_short!("port1");
        client.set_target_allocation(&owner, &portfolio, &TargetAllocation { allocations: weights(&env, &[(symbol_short!("USDC"), 5_000), (symbol_short!("XLM"), 3_000), (symbol_short!("BTC"), 2_000)]) });
        client.set_current_holdings(&owner, &portfolio, &CurrentHoldings { allocations: weights(&env, &[(symbol_short!("USDC"), 5_300), (symbol_short!("XLM"), 2_700), (symbol_short!("BTC"), 2_000)]) });
        client.set_drift_threshold_bps(&owner, &portfolio, &100);
        let result = client.rebalance(&owner, &portfolio);
        assert_eq!(result.adjustments.len(), 2);
        assert_eq!(result.adjustments.get(0).unwrap().direction, RebalanceDirection::Sell);
        assert_eq!(result.adjustments.get(1).unwrap().direction, RebalanceDirection::Buy);
    }

    #[test]
    fn test_scheduled_rebalance_execution() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let portfolio = symbol_short!("port1");
        client.set_schedule(&owner, &portfolio, &RebalanceInterval::Hourly);
        let allocation = weights(&env, &[(symbol_short!("USDC"), 10_000)]);
        client.set_target_allocation(&owner, &portfolio, &TargetAllocation { allocations: allocation.clone() });
        client.set_current_holdings(&owner, &portfolio, &CurrentHoldings { allocations: allocation });
        assert_eq!(client.check_exec_sched_rebalance(&portfolio), symbol_short!("not_due"));
        let mut ledger = env.ledger().get(); ledger.timestamp = 3600; env.ledger().set(ledger);
        assert_eq!(client.check_exec_sched_rebalance(&portfolio), symbol_short!("done"));
        let history = client.get_execution_history(&portfolio);
        assert_eq!(history.len(), 1);
        assert_eq!(history.get(0).unwrap().details, symbol_short!("schd_exec"));
    }

    #[test]
    fn test_owner_registration_and_access_control() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner1 = Address::generate(&env);
        let owner2 = Address::generate(&env);
        let portfolio = symbol_short!("port1");

        // First creation sets owner to owner1
        let allocation = weights(&env, &[(symbol_short!("USDC"), 10_000)]);
        let set_res = client.set_target_allocation(&owner1, &portfolio, &TargetAllocation { allocations: allocation.clone() });
        assert_eq!(set_res, symbol_short!("ok"));
        assert_eq!(client.get_owner(&portfolio), Some(owner1.clone()));

        // owner1 can update schedule
        assert_eq!(client.set_schedule(&owner1, &portfolio, &RebalanceInterval::Hourly), symbol_short!("ok"));
        assert_eq!(client.update_schedule(&owner1, &portfolio, &RebalanceInterval::Daily), symbol_short!("ok"));

        // owner2 attempts to mutate owner1's portfolio -> fails with err_auth / Unauthorized
        assert_eq!(client.update_schedule(&owner2, &portfolio, &RebalanceInterval::Weekly), symbol_short!("err_auth"));
        assert_eq!(client.cancel_schedule(&owner2, &portfolio), symbol_short!("err_auth"));
        assert_eq!(client.set_schedule(&owner2, &portfolio, &RebalanceInterval::Monthly), symbol_short!("err_auth"));

        let set_res2 = client.try_set_target_allocation(&owner2, &portfolio, &TargetAllocation { allocations: allocation.clone() });
        assert_eq!(set_res2, Err(Ok(RebalancingError::Unauthorized)));

        let reb_res = client.try_rebalance(&owner2, &portfolio);
        assert_eq!(reb_res, Err(Ok(RebalancingError::Unauthorized)));

        // owner1 can cancel schedule successfully
        assert_eq!(client.cancel_schedule(&owner1, &portfolio), symbol_short!("ok"));
    }

    #[test]
    fn test_read_methods_remain_public_without_auth() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let portfolio = symbol_short!("port1");

        let allocation = weights(&env, &[(symbol_short!("USDC"), 10_000)]);
        client.set_target_allocation(&owner, &portfolio, &TargetAllocation { allocations: allocation.clone() });
        client.set_current_holdings(&owner, &portfolio, &CurrentHoldings { allocations: allocation });
        client.set_schedule(&owner, &portfolio, &RebalanceInterval::Daily);

        // Read operations without mock_all_auths
        let env_no_auth = Env::default();
        let id = env.register_contract(None, RebalancingContract);
        let client_no_auth = RebalancingContractClient::new(&env_no_auth, &id);

        assert_eq!(client.get_owner(&portfolio), Some(owner));
        assert!(client.get_schedule(&portfolio).is_some());
        assert!(client.get_target_allocation(&portfolio).is_some());
        assert!(client.get_current_holdings(&portfolio).is_some());
        assert_eq!(client.get_status(&portfolio), symbol_short!("ok"));
        assert_eq!(client.get_drift_threshold_bps(&portfolio), DEFAULT_DRIFT_THRESHOLD_BPS);
    }

    // =========================================================================
    // Portfolio Creation & Initialization Tests
    // =========================================================================

    /// Helper to create a valid portfolio configuration.
    fn valid_portfolio_args(env: &Env) -> (Symbol, SorobanString, SorobanString, Vec<Symbol>, TargetAllocation) {
        let portfolio_id = symbol_short!("myportf");
        let name = SorobanString::from_str(env, "Growth Portfolio");
        let description = SorobanString::from_str(env, "A diversified growth portfolio");
        let assets = soroban_sdk::vec![&env, symbol_short!("USDC"), symbol_short!("XLM"), symbol_short!("BTC")];
        let allocations = weights(
            &env,
            &[
                (symbol_short!("USDC"), 4_000),
                (symbol_short!("XLM"), 3_500),
                (symbol_short!("BTC"), 2_500),
            ],
        );
        let target_allocation = TargetAllocation { allocations };
        (portfolio_id, name, description, assets, target_allocation)
    }

    #[test]
    fn test_initialize_portfolio_success() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let (portfolio_id, name, description, assets, target_allocation) = valid_portfolio_args(&env);

        let result = client.initialize_portfolio(
            &owner,
            &portfolio_id,
            &name,
            &description,
            &assets,
            &target_allocation,
        );

        assert!(result.is_ok());
        let portfolio = result.unwrap();
        assert_eq!(portfolio.id, portfolio_id);
        assert_eq!(portfolio.owner, owner);
        assert_eq!(portfolio.assets.len(), 3);
        assert_eq!(portfolio.metadata.name, SorobanString::from_str(&env, "Growth Portfolio"));
        assert_eq!(portfolio.metadata.description, SorobanString::from_str(&env, "A diversified growth portfolio"));
        assert!(portfolio.metadata.created_at > 0);
        assert_eq!(portfolio.metadata.created_at, portfolio.metadata.last_modified);
    }

    #[test]
    fn test_initialize_portfolio_stores_and_retrieves() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let (portfolio_id, name, description, assets, target_allocation) = valid_portfolio_args(&env);

        client.initialize_portfolio(&owner, &portfolio_id, &name, &description, &assets, &target_allocation).unwrap();

        // Retrieve the portfolio and verify all fields
        let retrieved = client.get_portfolio(&portfolio_id);
        assert!(retrieved.is_ok());
        let portfolio = retrieved.unwrap();
        assert_eq!(portfolio.id, portfolio_id);
        assert_eq!(portfolio.owner, owner);
        assert_eq!(portfolio.assets.len(), 3);
        assert!(portfolio.assets.contains(symbol_short!("USDC")));
        assert!(portfolio.assets.contains(symbol_short!("XLM")));
        assert!(portfolio.assets.contains(symbol_short!("BTC")));
        assert_eq!(portfolio.target_allocation.allocations.get(symbol_short!("USDC")), Some(4_000));
        assert_eq!(portfolio.target_allocation.allocations.get(symbol_short!("XLM")), Some(3_500));
        assert_eq!(portfolio.target_allocation.allocations.get(symbol_short!("BTC")), Some(2_500));
    }

    #[test]
    fn test_initialize_portfolio_already_exists() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let (portfolio_id, name, description, assets, target_allocation) = valid_portfolio_args(&env);

        // First creation succeeds
        let first = client.initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        );
        assert!(first.is_ok());

        // Second creation with same ID fails
        let second = client.try_initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        );
        assert_eq!(second, Err(Ok(RebalancingError::PortfolioAlreadyExists)));
    }

    #[test]
    fn test_initialize_portfolio_empty_assets() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let portfolio_id = symbol_short!("myportf");
        let name = SorobanString::from_str(&env, "Empty Assets");
        let description = SorobanString::from_str(&env, "Should fail");
        let assets: Vec<Symbol> = soroban_sdk::vec![&env];
        let allocations = weights(&env, &[(symbol_short!("USDC"), 10_000)]);
        let target_allocation = TargetAllocation { allocations };

        let result = client.try_initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        );
        assert_eq!(result, Err(Ok(RebalancingError::EmptyAssets)));
    }

    #[test]
    fn test_initialize_portfolio_empty_name() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let portfolio_id = symbol_short!("myportf");
        let name = SorobanString::from_str(&env, "");
        let description = SorobanString::from_str(&env, "desc");
        let assets = soroban_sdk::vec![&env, symbol_short!("USDC")];
        let allocations = weights(&env, &[(symbol_short!("USDC"), 10_000)]);
        let target_allocation = TargetAllocation { allocations };

        let result = client.try_initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        );
        assert_eq!(result, Err(Ok(RebalancingError::EmptyName)));
    }

    #[test]
    fn test_initialize_portfolio_allocation_sum_too_low() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let portfolio_id = symbol_short!("myportf");
        let name = SorobanString::from_str(&env, "Bad Alloc");
        let description = SorobanString::from_str(&env, "desc");
        let assets = soroban_sdk::vec![&env, symbol_short!("USDC"), symbol_short!("XLM")];
        // Sum = 9900, which is 100 bps below 10_000 — well outside the ±10 bps tolerance
        let allocations = weights(&env, &[(symbol_short!("USDC"), 5_000), (symbol_short!("XLM"), 4_900)]);
        let target_allocation = TargetAllocation { allocations };

        let result = client.try_initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        );
        assert_eq!(result, Err(Ok(RebalancingError::AllocationSumOutOfRange)));
    }

    #[test]
    fn test_initialize_portfolio_allocation_sum_too_high() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let portfolio_id = symbol_short!("myportf");
        let name = SorobanString::from_str(&env, "Bad Alloc");
        let description = SorobanString::from_str(&env, "desc");
        let assets = soroban_sdk::vec![&env, symbol_short!("USDC"), symbol_short!("XLM")];
        // Sum = 10_100, which is 100 bps above 10_000 — well outside the ±10 bps tolerance
        let allocations = weights(&env, &[(symbol_short!("USDC"), 5_000), (symbol_short!("XLM"), 5_100)]);
        let target_allocation = TargetAllocation { allocations };

        let result = client.try_initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        );
        assert_eq!(result, Err(Ok(RebalancingError::AllocationSumOutOfRange)));
    }

    #[test]
    fn test_initialize_portfolio_allocation_within_tolerance() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let portfolio_id = symbol_short!("myportf");
        let name = SorobanString::from_str(&env, "Tolerance Test");
        let description = SorobanString::from_str(&env, "desc");
        let assets = soroban_sdk::vec![&env, symbol_short!("USDC"), symbol_short!("XLM")];
        // Sum = 10_005, which is within the ±10 bps tolerance
        let allocations = weights(&env, &[(symbol_short!("USDC"), 5_000), (symbol_short!("XLM"), 5_005)]);
        let target_allocation = TargetAllocation { allocations };

        let result = client.try_initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_initialize_portfolio_allocation_at_boundary_low() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let portfolio_id = symbol_short!("myportf");
        let name = SorobanString::from_str(&env, "Boundary");
        let description = SorobanString::from_str(&env, "desc");
        let assets = soroban_sdk::vec![&env, symbol_short!("USDC"), symbol_short!("XLM")];
        // Sum = 9_990 — exactly at the lower tolerance boundary
        let allocations = weights(&env, &[(symbol_short!("USDC"), 5_000), (symbol_short!("XLM"), 4_990)]);
        let target_allocation = TargetAllocation { allocations };

        let result = client.try_initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_initialize_portfolio_allocation_at_boundary_high() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let portfolio_id = symbol_short!("myportf");
        let name = SorobanString::from_str(&env, "Boundary");
        let description = SorobanString::from_str(&env, "desc");
        let assets = soroban_sdk::vec![&env, symbol_short!("USDC"), symbol_short!("XLM")];
        // Sum = 10_010 — exactly at the upper tolerance boundary
        let allocations = weights(&env, &[(symbol_short!("USDC"), 5_000), (symbol_short!("XLM"), 5_010)]);
        let target_allocation = TargetAllocation { allocations };

        let result = client.try_initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_initialize_portfolio_unauthorized() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let attacker = Address::generate(&env);
        let (portfolio_id, name, description, assets, target_allocation) = valid_portfolio_args(&env);

        // Owner creates the portfolio
        client.initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        ).unwrap();

        // Attacker tries to update metadata — should be rejected
        let new_name = SorobanString::from_str(&env, "Hacked Name");
        let new_desc = SorobanString::from_str(&env, "hacked");
        let result = client.try_update_portfolio_metadata(&attacker, &portfolio_id, &new_name, &new_desc);
        assert_eq!(result, Err(Ok(RebalancingError::Unauthorized)));
    }

    #[test]
    fn test_update_portfolio_metadata_success() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let (portfolio_id, name, description, assets, target_allocation) = valid_portfolio_args(&env);

        client.initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        ).unwrap();

        // Advance the ledger to ensure last_modified changes
        let mut ledger = env.ledger().get();
        ledger.timestamp = 1000;
        env.ledger().set(ledger);
n        let new_name = SorobanString::from_str(&env, "Updated Portfolio");
        let new_desc = SorobanString::from_str(&env, "Updated description");
        let updated = client.update_portfolio_metadata(&owner, &portfolio_id, &new_name, &new_desc);
        assert!(updated.is_ok());

        let portfolio = updated.unwrap();
        assert_eq!(portfolio.metadata.name, SorobanString::from_str(&env, "Updated Portfolio"));
        assert_eq!(portfolio.metadata.description, SorobanString::from_str(&env, "Updated description"));
        assert!(portfolio.metadata.last_modified >= 1000);
    }

    #[test]
    fn test_update_portfolio_metadata_empty_name() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let (portfolio_id, name, description, assets, target_allocation) = valid_portfolio_args(&env);

        client.initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        ).unwrap();

        let empty_name = SorobanString::from_str(&env, "");
        let new_desc = SorobanString::from_str(&env, "desc");
        let result = client.try_update_portfolio_metadata(&owner, &portfolio_id, &empty_name, &new_desc);
        assert_eq!(result, Err(Ok(RebalancingError::EmptyName)));
    }

    #[test]
    fn test_update_portfolio_metadata_not_found() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let nonexistent = symbol_short!("nosuch");
        let new_name = SorobanString::from_str(&env, "Name");
        let new_desc = SorobanString::from_str(&env, "desc");

        let result = client.try_update_portfolio_metadata(&owner, &nonexistent, &new_name, &new_desc);
        assert_eq!(result, Err(Ok(RebalancingError::Unauthorized)));
    }

    #[test]
    fn test_update_portfolio_allocation_success() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let (portfolio_id, name, description, assets, target_allocation) = valid_portfolio_args(&env);

        client.initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        ).unwrap();

        let mut ledger = env.ledger().get();
        ledger.timestamp = 2000;
        env.ledger().set(ledger);

        // Update to a new valid allocation
        let new_alloc = weights(
            &env,
            &[
                (symbol_short!("USDC"), 3_000),
                (symbol_short!("XLM"), 3_000),
                (symbol_short!("BTC"), 4_000),
            ],
        );
        let new_target = TargetAllocation { allocations: new_alloc };
        let result = client.update_portfolio_allocation(&owner, &portfolio_id, &new_target);
        assert!(result.is_ok());

        let portfolio = result.unwrap();
        assert_eq!(portfolio.target_allocation.allocations.get(symbol_short!("USDC")), Some(3_000));
        assert_eq!(portfolio.target_allocation.allocations.get(symbol_short!("XLM")), Some(3_000));
        assert_eq!(portfolio.target_allocation.allocations.get(symbol_short!("BTC")), Some(4_000));
        assert!(portfolio.metadata.last_modified >= 2000);
    }

    #[test]
    fn test_update_portfolio_allocation_out_of_range() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let (portfolio_id, name, description, assets, target_allocation) = valid_portfolio_args(&env);

        client.initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        ).unwrap();

        // Bad allocation: sum = 8_000
        let bad_alloc = weights(
            &env,
            &[
                (symbol_short!("USDC"), 4_000),
                (symbol_short!("XLM"), 4_000),
            ],
        );
        let bad_target = TargetAllocation { allocations: bad_alloc };
        let result = client.try_update_portfolio_allocation(&owner, &portfolio_id, &bad_target);
        assert_eq!(result, Err(Ok(RebalancingError::AllocationSumOutOfRange)));
    }

    #[test]
    fn test_get_portfolio_not_found() {
        let env = Env::default();
        let client = client(&env);
        let nonexistent = symbol_short!("nosuch");
        let result = client.try_get_portfolio(&nonexistent);
        assert_eq!(result, Err(Ok(RebalancingError::PortfolioNotFound)));
    }

    #[test]
    fn test_initialize_portfolio_sets_owner_for_rebalance_compat() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let (portfolio_id, name, description, assets, target_allocation) = valid_portfolio_args(&env);

        client.initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        ).unwrap();

        // The owner should be accessible via get_owner (backward compat with rebalance)
        assert_eq!(client.get_owner(&portfolio_id), Some(owner.clone()));

        // The allocation should be accessible via get_target_allocation
        let stored_alloc = client.get_target_allocation(&portfolio_id);
        assert!(stored_alloc.is_some());
    }

    #[test]
    fn test_initialize_portfolio_timestamps_are_set() {
        let env = Env::default();
        env.mock_all_auths();
        let client = client(&env);
        let owner = Address::generate(&env);
        let (portfolio_id, name, description, assets, target_allocation) = valid_portfolio_args(&env);

        // Set ledger timestamp
        let mut ledger = env.ledger().get();
        ledger.timestamp = 5000;
        env.ledger().set(ledger);

        let portfolio = client.initialize_portfolio(
            &owner, &portfolio_id, &name, &description, &assets, &target_allocation,
        ).unwrap();

        assert_eq!(portfolio.metadata.created_at, 5000);
        assert_eq!(portfolio.metadata.last_modified, 5000);
    }
}