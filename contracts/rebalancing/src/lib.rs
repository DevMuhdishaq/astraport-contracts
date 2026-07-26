#![no_std]
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Map, Symbol, Vec};

use astraport_audit::logger::AuditLogger;
use astraport_audit::records::{permissions, AuditEventType, StateSnapshot};

/// Default tolerance used when deciding whether a holding needs rebalancing.
const DEFAULT_DRIFT_THRESHOLD_BPS: u32 = 100;

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
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RebalanceInterval {
    Hourly,
    Daily,
    Weekly,
    Monthly,
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

    /// Compute a rebalance plan from the stored target allocation and current
    /// holdings. The plan only includes assets whose absolute drift is greater
    /// than the configured threshold. A manual rebalance is recorded in the
    /// execution history.
    pub fn rebalance(
        env: Env,
        portfolio_id: Symbol,
    ) -> Result<RebalanceResult, RebalancingError> {
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

    pub fn set_schedule(env: Env, portfolio_id: Symbol, interval: RebalanceInterval) -> Symbol {
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

    pub fn update_schedule(env: Env, portfolio_id: Symbol, interval: RebalanceInterval) -> Symbol {
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

    pub fn cancel_schedule(env: Env, portfolio_id: Symbol) -> Symbol {
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
    /// * `portfolio_id` - Identifier for the portfolio
    /// * `allocation` - Target allocation with asset→basis-points weights
    ///
    /// # Returns
    /// `Ok(ok)` if the allocation is valid (sums to 10_000 bps) and persisted.
    /// `Err(RebalancingError::InvalidAllocation)` if weights don't sum to 10_000.
    pub fn set_target_allocation(
        env: Env,
        portfolio_id: Symbol,
        allocation: TargetAllocation,
    ) -> Result<Symbol, RebalancingError> {
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
        portfolio_id: Symbol,
        holdings: CurrentHoldings,
    ) -> Result<Symbol, RebalancingError> {
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
    pub fn set_drift_threshold_bps(env: Env, portfolio_id: Symbol, threshold_bps: u32) {
        let key = DataKey::DriftThreshold(portfolio_id);
        env.storage().persistent().set(&key, &threshold_bps);
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
        portfolio_id: Symbol,
        strategy: multi_asset_rebalancer::ExecutionStrategy,
    ) -> Result<(), RebalancingError> {
        let rebalancer_id = env.register_contract(None, multi_asset_rebalancer::MultiAssetRebalancer);
        let client = multi_asset_rebalancer::MultiAssetRebalancerClient::new(&env, &rebalancer_id);
        client.rebalance(&portfolio_id, &strategy);
        Ok(())
    }

    pub fn simulate_rebalance(
        env: Env,
        portfolio_id: Symbol,
        strategy: multi_asset_rebalancer::ExecutionStrategy,
    ) -> Result<multi_asset_rebalancer::SimulationResult, RebalancingError> {
        let rebalancer_id = env.register_contract(None, multi_asset_rebalancer::MultiAssetRebalancer);
        let client = multi_asset_rebalancer::MultiAssetRebalancerClient::new(&env, &rebalancer_id);
        Ok(client.simulate_rebalance(&portfolio_id, &strategy))
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
                before.push(k, *v as i128);
            }
            let mut after = StateSnapshot::empty(env);
            for (k, v) in balances_after.iter() {
                after.push(k, *v as i128);
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
    use soroban_sdk::{symbol_short, testutils::Ledger, Env, Map};


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
        let env = Env::default(); let client = client(&env); let portfolio = symbol_short!("port1");
        let allocation = weights(&env, &[(symbol_short!("USDC"), 6_000), (symbol_short!("XLM"), 4_000)]);
        client.set_target_allocation(&portfolio, &TargetAllocation { allocations: allocation.clone() });
        client.set_current_holdings(&portfolio, &CurrentHoldings { allocations: allocation });
        let result = client.rebalance(&portfolio);
        assert_eq!(result.adjustments.len(), 0);
        let history = client.get_execution_history(&portfolio);
        assert_eq!(history.len(), 1);
        assert_eq!(history.get(0).unwrap().details, symbol_short!("manual"));
    }

    #[test]
    fn test_rebalance_flags_single_asset_drift_with_direction() {
        let env = Env::default(); let client = client(&env); let portfolio = symbol_short!("port1");
        client.set_target_allocation(&portfolio, &TargetAllocation { allocations: weights(&env, &[(symbol_short!("USDC"), 5_000), (symbol_short!("XLM"), 3_000), (symbol_short!("BTC"), 2_000)]) });
        client.set_current_holdings(&portfolio, &CurrentHoldings { allocations: weights(&env, &[(symbol_short!("USDC"), 5_250), (symbol_short!("XLM"), 2_900), (symbol_short!("BTC"), 1_850)]) });
        client.set_drift_threshold_bps(&portfolio, &200);
        let result = client.rebalance(&portfolio);
        assert_eq!(result.adjustments.len(), 1);
        let adjustment = result.adjustments.get(0).unwrap();
        assert_eq!(adjustment.asset, symbol_short!("USDC"));
        assert_eq!(adjustment.drift_bps, 250);
        assert_eq!(adjustment.direction, RebalanceDirection::Sell);
    }

    #[test]
    fn test_rebalance_flags_multiple_assets_and_includes_buy_and_sell() {
        let env = Env::default(); let client = client(&env); let portfolio = symbol_short!("port1");
        client.set_target_allocation(&portfolio, &TargetAllocation { allocations: weights(&env, &[(symbol_short!("USDC"), 5_000), (symbol_short!("XLM"), 3_000), (symbol_short!("BTC"), 2_000)]) });
        client.set_current_holdings(&portfolio, &CurrentHoldings { allocations: weights(&env, &[(symbol_short!("USDC"), 5_300), (symbol_short!("XLM"), 2_700), (symbol_short!("BTC"), 2_000)]) });
        client.set_drift_threshold_bps(&portfolio, &100);
        let result = client.rebalance(&portfolio);
        assert_eq!(result.adjustments.len(), 2);
        assert_eq!(result.adjustments.get(0).unwrap().direction, RebalanceDirection::Sell);
        assert_eq!(result.adjustments.get(1).unwrap().direction, RebalanceDirection::Buy);
    }

    #[test]
    fn test_scheduled_rebalance_execution() {
        let env = Env::default(); let client = client(&env); let portfolio = symbol_short!("port1");
        client.set_schedule(&portfolio, &RebalanceInterval::Hourly);
        let allocation = weights(&env, &[(symbol_short!("USDC"), 10_000)]);
        client.set_target_allocation(&portfolio, &TargetAllocation { allocations: allocation.clone() });
        client.set_current_holdings(&portfolio, &CurrentHoldings { allocations: allocation });
        assert_eq!(client.check_exec_sched_rebalance(&portfolio), symbol_short!("not_due"));
        let mut ledger = env.ledger().get(); ledger.timestamp = 3600; env.ledger().set(ledger);
        assert_eq!(client.check_exec_sched_rebalance(&portfolio), symbol_short!("done"));
        let history = client.get_execution_history(&portfolio);
        assert_eq!(history.len(), 1);
        assert_eq!(history.get(0).unwrap().details, symbol_short!("schd_exec"));
    }
}