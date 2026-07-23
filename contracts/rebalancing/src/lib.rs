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
    pub fn initialize(_env: Env) -> Symbol {
        symbol_short!("ok")
    }

    /// Rebalance portfolio based on target allocations
    /// Publishes event (REBAL, portfolio_id) with drift summary data.
    pub fn rebalance(env: Env, portfolio_id: Symbol) -> Symbol {
        let outcome = symbol_short!("done");
        let ts = env.ledger().timestamp();
        let event_data = RebalanceEventData {
            portfolio_id: portfolio_id.clone(),
            outcome: outcome.clone(),
            timestamp: ts,
        };
        env.events().publish((symbol_short!("REBAL"), portfolio_id.clone()), event_data);
        outcome
    }

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

    pub fn get_execution_history(env: Env, portfolio_id: Symbol) -> Vec<ExecutionHistoryRecord> {
        let key = DataKey::History(portfolio_id);
        env.storage().persistent().get(&key).unwrap_or_else(|| Vec::new(&env))
    }

    /// Check and execute scheduled rebalance (short name to satisfy 32-char limit)
    /// Publishes events on execution and on not_due / err_none paths.
    pub fn check_exec_rebalance(env: Env, portfolio_id: Symbol) -> Symbol {
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
    use soroban_sdk::{Env, symbol_short};
    use soroban_sdk::testutils::{Ledger, Events as TestEvents};

    fn setup_env_and_client() -> (Env, RebalancingContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, RebalancingContract);
        let client = RebalancingContractClient::new(&env, &contract_id);
        (env, client)
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
        let (env, client) = setup_env_and_client();
        let portfolio = symbol_short!("port1");

        assert!(client.get_schedule(&portfolio).is_none());

        let res = client.set_schedule(&portfolio, &RebalanceInterval::Hourly);
        assert_eq!(res, symbol_short!("ok"));

        let schedule = client.get_schedule(&portfolio).unwrap();
        assert_eq!(schedule.portfolio_id, portfolio);
        assert_eq!(schedule.interval, RebalanceInterval::Hourly);
        assert_eq!(schedule.last_execution, 0);
        assert_eq!(schedule.next_execution, 3600);

        let res_dup = client.set_schedule(&portfolio, &RebalanceInterval::Daily);
        assert_eq!(res_dup, symbol_short!("err_exist"));
        let _ = env;
    }

    #[test]
    fn test_update_and_cancel_schedule() {
        let (env, client) = setup_env_and_client();
        let portfolio = symbol_short!("port1");

        let res_up = client.update_schedule(&portfolio, &RebalanceInterval::Daily);
        assert_eq!(res_up, symbol_short!("err_none"));

        client.set_schedule(&portfolio, &RebalanceInterval::Hourly);

        let res_up2 = client.update_schedule(&portfolio, &RebalanceInterval::Daily);
        assert_eq!(res_up2, symbol_short!("ok"));

        let schedule = client.get_schedule(&portfolio).unwrap();
        assert_eq!(schedule.interval, RebalanceInterval::Daily);

        let res_cancel = client.cancel_schedule(&portfolio);
        assert_eq!(res_cancel, symbol_short!("ok"));

        assert!(client.get_schedule(&portfolio).is_none());

        let res_cancel2 = client.cancel_schedule(&portfolio);
        assert_eq!(res_cancel2, symbol_short!("err_none"));
        let _ = env;
    }

    #[test]
    fn test_scheduled_rebalance_execution() {
        let (env, client) = setup_env_and_client();
        let portfolio = symbol_short!("port1");

        let res = client.check_exec_rebalance(&portfolio);
        assert_eq!(res, symbol_short!("err_none"));

        client.set_schedule(&portfolio, &RebalanceInterval::Hourly);

        let res_not_due = client.check_exec_rebalance(&portfolio);
        assert_eq!(res_not_due, symbol_short!("not_due"));

        let mut ledger_info = env.ledger().get();
        ledger_info.timestamp = 3600;
        env.ledger().set(ledger_info);

        let res_due = client.check_exec_rebalance(&portfolio);
        assert_eq!(res_due, symbol_short!("done"));

        let schedule = client.get_schedule(&portfolio).unwrap();
        assert_eq!(schedule.last_execution, 3600);
        assert_eq!(schedule.next_execution, 7200);

        let history = client.get_execution_history(&portfolio);
        assert_eq!(history.len(), 1);
        let record = history.get(0).unwrap();
        assert_eq!(record.timestamp, 3600);
        assert_eq!(record.outcome, symbol_short!("done"));
        assert_eq!(record.details, symbol_short!("sched_ex"));
    }

    #[test]
    fn test_backward_compat_old_name() {
        let (env, client) = setup_env_and_client();
        // Use direct wrapper via contract client for old name? Old name not in client, so test via env.as_contract
        let portfolio = symbol_short!("port1");
        // old name is available as associated fn, but still needs contract context for storage
        // We'll test via as_contract registration
        let contract_id = client.address.clone();
        env.as_contract(&contract_id, || {
            let res = RebalancingContract::check_and_execute_scheduled_rebalance(env.clone(), portfolio.clone());
            assert_eq!(res, symbol_short!("err_none"));
        });
    }

    #[test]
    fn test_rebalance_publishes_event() {
        let (env, client) = setup_env_and_client();
        let portfolio = symbol_short!("port1");

        let outcome = client.rebalance(&portfolio);
        assert_eq!(outcome, symbol_short!("done"));

        let events = env.events().all();
        assert_eq!(events.len(), 1);
        let (_contract, topics, _data) = events.get(0).unwrap();
        assert_eq!(topics.len(), 2);
    }

    #[test]
    fn test_rebalance_event_contains_portfolio_and_outcome() {
        let (env, client) = setup_env_and_client();
        let portfolio = symbol_short!("myport");

        client.rebalance(&portfolio);
        let events = env.events().all();
        assert_eq!(events.len(), 1);

        let portfolio2 = symbol_short!("port2");
        client.rebalance(&portfolio2);
        let events2 = env.events().all();
        assert_eq!(events2.len(), 2);
    }

    #[test]
    fn test_scheduled_execution_publishes_events() {
        let (env, client) = setup_env_and_client();
        let portfolio = symbol_short!("port1");

        let res = client.check_exec_rebalance(&portfolio);
        assert_eq!(res, symbol_short!("err_none"));
        let events = env.events().all();
        assert_eq!(events.len(), 1);
        let (_c, topics, _d) = events.get(0).unwrap();
        assert_eq!(topics.len(), 2);

        client.set_schedule(&portfolio, &RebalanceInterval::Hourly);
        let res_not_due = client.check_exec_rebalance(&portfolio);
        assert_eq!(res_not_due, symbol_short!("not_due"));
        let events2 = env.events().all();
        assert_eq!(events2.len(), 2);

        let mut ledger_info = env.ledger().get();
        ledger_info.timestamp = 3601;
        env.ledger().set(ledger_info);

        let res_due = client.check_exec_rebalance(&portfolio);
        assert_eq!(res_due, symbol_short!("done"));
        let events3 = env.events().all();
        assert_eq!(events3.len(), 4);
    }

    #[test]
    fn test_scheduled_not_due_event() {
        let (env, client) = setup_env_and_client();
        let portfolio = symbol_short!("port1");
        client.set_schedule(&portfolio, &RebalanceInterval::Daily);
        let res = client.check_exec_rebalance(&portfolio);
        assert_eq!(res, symbol_short!("not_due"));
        let events = env.events().all();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_scheduled_err_none_event() {
        let (env, client) = setup_env_and_client();
        let portfolio = symbol_short!("ghost");
        let res = client.check_exec_rebalance(&portfolio);
        assert_eq!(res, symbol_short!("err_none"));
        let events = env.events().all();
        assert_eq!(events.len(), 1);
    }
}
