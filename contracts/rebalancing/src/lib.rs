#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Env, Symbol, Vec};

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

#[contracttype]
pub enum DataKey {
    Schedule(Symbol),
    History(Symbol),
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
    pub fn initialize(env: Env) -> Symbol {
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
    pub fn rebalance(env: Env, portfolio_id: Symbol) -> Symbol {
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
    pub fn get_status(env: Env, portfolio_id: Symbol) -> Symbol {
        symbol_short!("ok")
    }

    /// Set a schedule for a portfolio
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

    /// Update schedule for a portfolio
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

    /// Cancel a schedule for a portfolio
    pub fn cancel_schedule(env: Env, portfolio_id: Symbol) -> Symbol {
        let key = DataKey::Schedule(portfolio_id.clone());
        if !env.storage().persistent().has(&key) {
            return symbol_short!("err_none");
        }
        env.storage().persistent().remove(&key);
        symbol_short!("ok")
    }

    /// Get schedule for a portfolio
    pub fn get_schedule(env: Env, portfolio_id: Symbol) -> Option<RebalancingSchedule> {
        let key = DataKey::Schedule(portfolio_id);
        env.storage().persistent().get(&key)
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
            return symbol_short!("err_none");
        }

        let mut schedule: RebalancingSchedule = env.storage().persistent().get(&key).unwrap();
        let now = env.ledger().timestamp();

        if now < schedule.next_execution {
            return symbol_short!("not_due");
        }

        // Execute rebalance
        let outcome = Self::rebalance(env.clone(), portfolio_id.clone());

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

        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{symbol_short, Env};

    #[test]
    fn test_initialize() {
        let env = Env::default();
        let result = RebalancingContract::initialize(env);
        assert_eq!(result, symbol_short!("ok"));
    }

    #[test]
    fn test_set_and_get_schedule() {
        let env = Env::default();
        let portfolio = symbol_short!("port1");

        assert!(RebalancingContract::get_schedule(env.clone(), portfolio.clone()).is_none());

        let res = RebalancingContract::set_schedule(
            env.clone(),
            portfolio.clone(),
            RebalanceInterval::Hourly,
        );
        assert_eq!(res, symbol_short!("ok"));

        let schedule = RebalancingContract::get_schedule(env.clone(), portfolio.clone()).unwrap();
        assert_eq!(schedule.portfolio_id, portfolio);
        assert_eq!(schedule.interval, RebalanceInterval::Hourly);
        assert_eq!(schedule.last_execution, 0);
        assert_eq!(schedule.next_execution, 3600);

        let res_dup = RebalancingContract::set_schedule(
            env.clone(),
            portfolio.clone(),
            RebalanceInterval::Daily,
        );
        assert_eq!(res_dup, symbol_short!("err_exist"));
    }

    #[test]
    fn test_update_and_cancel_schedule() {
        let env = Env::default();
        let portfolio = symbol_short!("port1");

        let res_up = RebalancingContract::update_schedule(
            env.clone(),
            portfolio.clone(),
            RebalanceInterval::Daily,
        );
        assert_eq!(res_up, symbol_short!("err_none"));

        RebalancingContract::set_schedule(
            env.clone(),
            portfolio.clone(),
            RebalanceInterval::Hourly,
        );

        let res_up2 = RebalancingContract::update_schedule(
            env.clone(),
            portfolio.clone(),
            RebalanceInterval::Daily,
        );
        assert_eq!(res_up2, symbol_short!("ok"));

        let schedule = RebalancingContract::get_schedule(env.clone(), portfolio.clone()).unwrap();
        assert_eq!(schedule.interval, RebalanceInterval::Daily);

        let res_cancel = RebalancingContract::cancel_schedule(env.clone(), portfolio.clone());
        assert_eq!(res_cancel, symbol_short!("ok"));

        assert!(RebalancingContract::get_schedule(env.clone(), portfolio.clone()).is_none());

        let res_cancel2 = RebalancingContract::cancel_schedule(env.clone(), portfolio.clone());
        assert_eq!(res_cancel2, symbol_short!("err_none"));
    }

    #[test]
    fn test_scheduled_rebalance_execution() {
        let env = Env::default();
        let portfolio = symbol_short!("port1");

        let res = RebalancingContract::check_and_execute_scheduled_rebalance(
            env.clone(),
            portfolio.clone(),
        );
        assert_eq!(res, symbol_short!("err_none"));

        RebalancingContract::set_schedule(
            env.clone(),
            portfolio.clone(),
            RebalanceInterval::Hourly,
        );

        let res_not_due = RebalancingContract::check_and_execute_scheduled_rebalance(
            env.clone(),
            portfolio.clone(),
        );
        assert_eq!(res_not_due, symbol_short!("not_due"));

        let mut ledger_info = env.ledger().get();
        ledger_info.timestamp = 3600;
        env.ledger().set(ledger_info);

        let res_due = RebalancingContract::check_and_execute_scheduled_rebalance(
            env.clone(),
            portfolio.clone(),
        );
        assert_eq!(res_due, symbol_short!("done"));

        let schedule = RebalancingContract::get_schedule(env.clone(), portfolio.clone()).unwrap();
        assert_eq!(schedule.last_execution, 3600);
        assert_eq!(schedule.next_execution, 7200);

        let history = RebalancingContract::get_execution_history(env.clone(), portfolio.clone());
        assert_eq!(history.len(), 1);
        let record = history.get(0).unwrap();
        assert_eq!(record.timestamp, 3600);
        assert_eq!(record.outcome, symbol_short!("done"));
        assert_eq!(record.details, symbol_short!("sched_exec"));
    }
}
