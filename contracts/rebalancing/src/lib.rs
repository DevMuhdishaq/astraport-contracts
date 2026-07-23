#![no_std]
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, symbol_short, Env, Map, Symbol, Vec};

/// Errors returned by the rebalancing contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RebalancingError {
    /// The target allocation weights do not sum to 10_000 basis points (100%).
    InvalidAllocation = 1,
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

#[contracttype]
pub enum DataKey {
    Schedule(Symbol),
    History(Symbol),
    Allocation(Symbol),
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
            RebalanceInterval::Hourly | RebalanceInterval::Daily | RebalanceInterval::Weekly | RebalanceInterval::Monthly => true,
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

    /// Rebalance portfolio based on target allocations
    /// 
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `portfolio_id` - Identifier for the portfolio to rebalance
    /// 
    /// # Returns
    /// Success symbol if rebalancing succeeds
    pub fn rebalance(_env: Env, _portfolio_id: Symbol) -> Symbol {
        symbol_short!("done")
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

    /// Get execution history for a portfolio
    pub fn get_execution_history(env: Env, portfolio_id: Symbol) -> Vec<ExecutionHistoryRecord> {
        let key = DataKey::History(portfolio_id);
        env.storage().persistent().get(&key).unwrap_or_else(|| Vec::new(&env))
    }

    /// Check and execute scheduled rebalance
    pub fn exec_scheduled_rebalance(env: Env, portfolio_id: Symbol) -> Symbol {
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
        let outcome = Self::rebalance(env.clone(), portfolio_id.clone());
        schedule.last_execution = now;
        schedule.next_execution = now + interval_to_seconds(&schedule.interval);
        env.storage().persistent().set(&key, &schedule);
        let history_key = DataKey::History(portfolio_id.clone());
        let mut history: Vec<ExecutionHistoryRecord> = env
            .storage()
            .persistent()
            .get(&history_key)
            .unwrap_or_else(|| Vec::new(&env));
        let record = ExecutionHistoryRecord {
            timestamp: now,
            outcome: outcome.clone(),
            details: symbol_short!("sched_ex"),
        };
        history.push_back(record);
        env.storage().persistent().set(&history_key, &history);
        let sched_event = SchedRebalanceEventData {
            portfolio_id: portfolio_id.clone(),
            outcome: outcome.clone(),
            timestamp: now,
            details: symbol_short!("sched_ex"),
        };
        env.events().publish((symbol_short!("SREBAL"), portfolio_id.clone()), sched_event);
        outcome
    }

    pub fn check_and_exec_sched(env: Env, portfolio_id: Symbol) -> Symbol {
        Self::check_exec_rebalance(env, portfolio_id)
    }
}

impl RebalancingContract {
    pub fn check_and_execute_scheduled_rebalance(env: Env, portfolio_id: Symbol) -> Symbol {
        Self::check_exec_rebalance(env, portfolio_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{Env, symbol_short, map};
    use soroban_sdk::testutils::Ledger;

    /// Register the contract and run a test closure inside `as_contract`.
    fn test_in_contract<T>(f: impl FnOnce(Env) -> T) -> T {
        let env = Env::default();
        let contract_id = env.register_contract(None, RebalancingContract);
        env.as_contract(&contract_id, || f(env.clone()))
    }

    #[test]
    fn test_initialize() {
        let env = Env::default();
        let contract_id = env.register_contract(None, RebalancingContract);
        let client = RebalancingContractClient::new(&env, &contract_id);
        let result = client.initialize();
        assert_eq!(result, symbol_short!("ok"));
    }

    #[test]
    fn test_set_and_get_schedule() {
        test_in_contract(|env| {
            let portfolio = symbol_short!("port1");

            assert!(RebalancingContract::get_schedule(env.clone(), portfolio.clone()).is_none());

            let res = RebalancingContract::set_schedule(env.clone(), portfolio.clone(), RebalanceInterval::Hourly);
            assert_eq!(res, symbol_short!("ok"));

            let schedule = RebalancingContract::get_schedule(env.clone(), portfolio.clone()).unwrap();
            assert_eq!(schedule.portfolio_id, portfolio);
            assert_eq!(schedule.interval, RebalanceInterval::Hourly);
            assert_eq!(schedule.last_execution, 0);
            assert_eq!(schedule.next_execution, 3600);

            let res_dup = RebalancingContract::set_schedule(env.clone(), portfolio.clone(), RebalanceInterval::Daily);
            assert_eq!(res_dup, symbol_short!("err_exist"));
        });
    }

    #[test]
    fn test_update_and_cancel_schedule() {
        test_in_contract(|env| {
            let portfolio = symbol_short!("port1");

            let res_up = RebalancingContract::update_schedule(env.clone(), portfolio.clone(), RebalanceInterval::Daily);
            assert_eq!(res_up, symbol_short!("err_none"));

            RebalancingContract::set_schedule(env.clone(), portfolio.clone(), RebalanceInterval::Hourly);

            let res_up2 = RebalancingContract::update_schedule(env.clone(), portfolio.clone(), RebalanceInterval::Daily);
            assert_eq!(res_up2, symbol_short!("ok"));

            let schedule = RebalancingContract::get_schedule(env.clone(), portfolio.clone()).unwrap();
            assert_eq!(schedule.interval, RebalanceInterval::Daily);

            let res_cancel = RebalancingContract::cancel_schedule(env.clone(), portfolio.clone());
            assert_eq!(res_cancel, symbol_short!("ok"));

            assert!(RebalancingContract::get_schedule(env.clone(), portfolio.clone()).is_none());

            let res_cancel2 = RebalancingContract::cancel_schedule(env.clone(), portfolio.clone());
            assert_eq!(res_cancel2, symbol_short!("err_none"));
        });
    }

    #[test]
    fn test_scheduled_rebalance_execution() {
        test_in_contract(|env| {
            let portfolio = symbol_short!("port1");

            let res = RebalancingContract::exec_scheduled_rebalance(env.clone(), portfolio.clone());
            assert_eq!(res, symbol_short!("err_none"));

            RebalancingContract::set_schedule(env.clone(), portfolio.clone(), RebalanceInterval::Hourly);

            let res_not_due = RebalancingContract::exec_scheduled_rebalance(env.clone(), portfolio.clone());
            assert_eq!(res_not_due, symbol_short!("not_due"));

            let mut ledger_info = env.ledger().get();
            ledger_info.timestamp = 3600;
            env.ledger().set(ledger_info);

            let res_due = RebalancingContract::exec_scheduled_rebalance(env.clone(), portfolio.clone());
            assert_eq!(res_due, symbol_short!("done"));

            let schedule = RebalancingContract::get_schedule(env.clone(), portfolio.clone()).unwrap();
            assert_eq!(schedule.last_execution, 3600);
            assert_eq!(schedule.next_execution, 7200);

            let history = RebalancingContract::get_execution_history(env.clone(), portfolio.clone());
            assert_eq!(history.len(), 1);
            let record = history.get(0).unwrap();
            assert_eq!(record.timestamp, 3600);
            assert_eq!(record.outcome, symbol_short!("done"));
            assert_eq!(record.details, symbol_short!("sched_ex"));
        });
    }

    // ── Target allocation tests ──

    #[test]
    fn test_set_and_get_valid_allocation() {
        test_in_contract(|env| {
            let portfolio = symbol_short!("port1");

            let allocs = map![&env, (symbol_short!("BTC"), 4000u32), (symbol_short!("ETH"), 3000u32), (symbol_short!("SOL"), 3000u32)];
            let allocation = TargetAllocation {
                allocations: allocs,
            };

            let result = RebalancingContract::set_target_allocation(
                env.clone(),
                portfolio.clone(),
                allocation,
            );
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), symbol_short!("ok"));

            let stored = RebalancingContract::get_target_allocation(env.clone(), portfolio.clone());
            assert!(stored.is_some());

            let stored_alloc = stored.unwrap();
            assert_eq!(stored_alloc.allocations.len(), 3);
            assert_eq!(stored_alloc.allocations.get(symbol_short!("BTC")).unwrap(), 4000);
            assert_eq!(stored_alloc.allocations.get(symbol_short!("ETH")).unwrap(), 3000);
            assert_eq!(stored_alloc.allocations.get(symbol_short!("SOL")).unwrap(), 3000);
        });
    }

    #[test]
    fn test_allocation_over_100_percent_rejected() {
        test_in_contract(|env| {
            let portfolio = symbol_short!("port1");

            // 60% + 55% = 115% → 11_500 bps, over the 10_000 limit
            let allocs = map![&env, (symbol_short!("BTC"), 6000u32), (symbol_short!("ETH"), 5500u32)];
            let allocation = TargetAllocation {
                allocations: allocs,
            };

            let result = RebalancingContract::set_target_allocation(
                env.clone(),
                portfolio.clone(),
                allocation,
            );
            assert!(result.is_err());
            assert_eq!(result.unwrap_err(), RebalancingError::InvalidAllocation);

            // Verify nothing was stored
            let stored = RebalancingContract::get_target_allocation(env.clone(), portfolio.clone());
            assert!(stored.is_none());
        });
    }

    #[test]
    fn test_allocation_under_100_percent_rejected() {
        test_in_contract(|env| {
            let portfolio = symbol_short!("port1");

            // 40% + 30% = 70% → 7_000 bps, under the 10_000 limit
            let allocs = map![&env, (symbol_short!("BTC"), 4000u32), (symbol_short!("ETH"), 3000u32)];
            let allocation = TargetAllocation {
                allocations: allocs,
            };

            let result = RebalancingContract::set_target_allocation(
                env.clone(),
                portfolio.clone(),
                allocation,
            );
            assert!(result.is_err());
            assert_eq!(result.unwrap_err(), RebalancingError::InvalidAllocation);

            // Verify nothing was stored
            let stored = RebalancingContract::get_target_allocation(env.clone(), portfolio.clone());
            assert!(stored.is_none());
        });
    }

    #[test]
    fn test_allocation_exactly_100_percent_accepted() {
        test_in_contract(|env| {
            let portfolio = symbol_short!("port1");

            // Single asset at 100%
            let allocs = map![&env, (symbol_short!("BTC"), 10000u32)];
            let allocation = TargetAllocation {
                allocations: allocs,
            };

            let result = RebalancingContract::set_target_allocation(
                env.clone(),
                portfolio.clone(),
                allocation,
            );
            assert!(result.is_ok());

            let stored = RebalancingContract::get_target_allocation(env.clone(), portfolio.clone());
            assert!(stored.is_some());
        });
    }

    #[test]
    fn test_get_allocation_returns_none_when_not_set() {
        test_in_contract(|env| {
            let portfolio = symbol_short!("port1");

            let stored = RebalancingContract::get_target_allocation(env.clone(), portfolio.clone());
            assert!(stored.is_none());
        });
    }
}
