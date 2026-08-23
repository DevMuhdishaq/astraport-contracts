//! # AstraPort Smart Contracts — Usage Examples
//!
//! Complete, runnable examples demonstrating common workflows for each contract.
//! These examples use the Soroban test environment and can be run with:
//!
//! ```bash
//! cargo test --package astraport-staking -- examples
//! cargo test --package astraport-rebalancing -- examples
//! cargo test --package astraport-events -- examples
//! cargo test --package astraport-audit -- examples
//! ```

use soroban_sdk::{symbol_short, testutils::Address as _, testutils::Ledger, Address, Env, Symbol};

// ===========================================================================
// Staking Contract Examples
// ===========================================================================

#[cfg(test)]
mod staking_examples {
    use super::*;
    use astraport_staking::StakingContract;
    use astraport_staking::StakingContractClient;
    use astraport_staking::records::CompoundingMode;

    fn setup() -> (Env, StakingContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, StakingContract);
        let client = StakingContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let _ = client.initialize(&admin);
        (env, client, admin)
    }

    // ----- Example 1: Basic Staking Lifecycle -----
    //
    // Demonstrates the complete stake → query → unstake flow.

    #[test]
    fn example_basic_staking_lifecycle() {
        let (env, client, _admin) = setup();
        let staker = Address::generate(&env);
        let xlm = symbol_short!("XLM");

        // 1. Stake 1,000 XLM (10,000,000 stroops each = 10,000,000,000)
        let amount = 10_000_000_000i128;
        let result = client.try_stake(&staker, &xlm, &amount);
        assert!(result.is_ok(), "Stake should succeed");
        assert_eq!(result.unwrap(), Ok(symbol_short!("ok")));

        // 2. Query balance — should match staked amount
        let balance = client.get_balance(&staker, &xlm);
        assert_eq!(balance, amount);

        // 3. Query protocol totals
        assert_eq!(client.total_staked(&xlm), amount);
        assert_eq!(client.staker_count(), 1);

        // 4. Unstake half
        let unstake_amount = amount / 2;
        let result = client.try_unstake(&staker, &xlm, &unstake_amount);
        assert!(result.is_ok());
        assert_eq!(client.get_balance(&staker, &xlm), amount - unstake_amount);
        assert_eq!(client.total_staked(&xlm), amount - unstake_amount);

        // 5. Unstake the rest — balance goes to 0, key is removed
        let result = client.try_unstake(&staker, &xlm, &(amount - unstake_amount));
        assert!(result.is_ok());
        assert_eq!(client.get_balance(&staker, &xlm), 0);
        assert_eq!(client.total_staked(&xlm), 0);
        assert_eq!(client.staker_count(), 0);
    }

    // ----- Example 2: Error Handling — Insufficient Balance -----
    //
    // Demonstrates proper error handling when trying to unstake more than staked.

    #[test]
    fn example_insufficient_balance_error() {
        let (env, client, _admin) = setup();
        let staker = Address::generate(&env);
        let xlm = symbol_short!("XLM");

        // Stake 100
        let _ = client.try_stake(&staker, &xlm, &100);

        // Try to unstake 200 — should fail with InsufficientBalance
        let result = client.try_unstake(&staker, &xlm, &200);
        assert_eq!(result, Err(Ok(astraport_staking::Error::InsufficientBalance)));
    }

    // ----- Example 3: Error Handling — Invalid Amount -----
    //
    // Demonstrates rejection of zero and negative stake amounts.

    #[test]
    fn example_invalid_stake_amount_error() {
        let (env, client, _admin) = setup();
        let staker = Address::generate(&env);
        let xlm = symbol_short!("XLM");

        // Try to stake zero
        let result = client.try_stake(&staker, &xlm, &0);
        assert_eq!(result, Err(Ok(astraport_staking::Error::InvalidStakeAmount)));

        // Try to stake negative
        let result = client.try_stake(&staker, &xlm, &-100);
        assert_eq!(result, Err(Ok(astraport_staking::Error::InvalidStakeAmount)));
    }

    // ----- Example 4: Multi-Asset Staking -----
    //
    // Demonstrates staking across multiple assets simultaneously.

    #[test]
    fn example_multi_asset_staking() {
        let (env, client, _admin) = setup();
        let staker = Address::generate(&env);
        let xlm = symbol_short!("XLM");
        let usdc = symbol_short!("USDC");

        // Stake both assets
        let _ = client.try_stake(&staker, &xlm, &1_000_000);
        let _ = client.try_stake(&staker, &usdc, &500_000);

        // Check per-asset balances
        assert_eq!(client.get_balance(&staker, &xlm), 1_000_000);
        assert_eq!(client.get_balance(&staker, &usdc), 500_000);

        // Protocol totals are tracked per-asset
        assert_eq!(client.total_staked(&xlm), 1_000_000);
        assert_eq!(client.total_staked(&usdc), 500_000);

        // Staker count is 1 (same address staking both)
        assert_eq!(client.staker_count(), 1);

        // Unstake USDC only
        let _ = client.try_unstake(&staker, &usdc, &500_000);
        assert_eq!(client.get_balance(&staker, &usdc), 0);
        assert_eq!(client.total_staked(&usdc), 0);

        // XLM balance unchanged
        assert_eq!(client.get_balance(&staker, &xlm), 1_000_000);
        assert_eq!(client.total_staked(&xlm), 1_000_000);
    }

    // ----- Example 5: Yield Position Management -----
    //
    // Demonstrates opening a yield position, accruing yield, and projecting earnings.

    #[test]
    fn example_yield_position_lifecycle() {
        let (env, client, _admin) = setup();
        let staker = Address::generate(&env);
        let xlm = symbol_short!("XLM");

        // Open a yield position: 10% APR, continuous compounding
        let apr = 100_000_000_000_000_000i128; // 10% in fixed-point (1e18 scale)
        let record = client.open_yield_position(&staker, &xlm, &1_000_000, &apr, &CompoundingMode::Continuous);

        // Verify position is created
        assert_eq!(record.principal, 1_000_000);
        assert_eq!(record.apr, apr);
        assert_eq!(record.mode, CompoundingMode::Continuous);

        // Advance ledger time by 30 days
        env.ledger().set_timestamp(env.ledger().timestamp() + 30 * 86400);

        // Accrue yield to the new timestamp
        let accrued = client.accrue_yield(&staker, &xlm);
        assert!(accrued.accrued_yield > 0, "Yield should accrue over time");

        // Query current yield (includes uncheckpointed portion)
        let current = client.current_yield(&staker, &xlm);
        assert!(current >= accrued.accrued_yield);

        // Project 90-day earnings
        let projection = client.project_yield(&1_000_000, &apr, &CompoundingMode::Continuous, &(90 * 86400));
        assert!(projection.projected_yield > 0);
        assert!(projection.projected_balance > 1_000_000);
    }

    // ----- Example 6: APR/APY Conversion -----
    //
    // Demonstrates rate conversion between APR and APY.

    #[test]
    fn example_apr_apy_conversion() {
        let (_env, client, _admin) = setup();

        // 5% APR → APY
        let apr_5pct = 50_000_000_000_000_000i128; // 0.05 * 1e18
        let apy_daily = client.apr_to_apy(&apr_5pct, &CompoundingMode::Daily);
        let apy_continuous = client.apr_to_apy(&apr_5pct, &CompoundingMode::Continuous);

        // APY should be slightly higher than APR due to compounding
        assert!(apy_daily > apr_5pct);
        assert!(apy_continuous >= apy_daily);

        // Round-trip: APY → APR should recover the original APR
        let recovered_apr = client.apy_to_apr(&apy_daily, &CompoundingMode::Daily);
        let diff = (recovered_apr - apr_5pct).abs();
        assert!(diff < 1_000_000, "Round-trip should be accurate within 0.0001%");
    }

    // ----- Example 7: Emergency Unstaking -----
    //
    // Demonstrates the full emergency unstake flow with penalty configuration.

    #[test]
    fn example_emergency_unstake_flow() {
        let (env, client, admin) = setup();
        let staker = Address::generate(&env);
        let treasury = Address::generate(&env);
        let xlm = symbol_short!("XLM");

        // 1. Configure emergency unstaking: 30% start penalty, 5% end penalty
        let _ = client.configure_emergency_unstake(
            &admin,
            &3_000,   // 30% penalty at lock start
            &500,     // 5% penalty at unlock
            &astraport_staking::emergency::PenaltyDecayFunction::Linear,
            &86_400,  // 1-day cooldown
            &treasury,
            &true,
        );

        // 2. Set a lock position: 30-day lock starting now
        let now = env.ledger().timestamp();
        let _ = client.set_lock_position(&admin, &staker, &now, &(now + 30 * 86400), &1_000_000);

        // 3. Stake assets
        let _ = client.try_stake(&staker, &xlm, &1_000_000);

        // 4. Preview penalty early in the lock (should be high)
        let penalty = client.preview_emergency_penalty(&now, &(now + 30 * 86400));
        assert!(penalty.unwrap() >= 2_500, "Early penalty should be significant");

        // 5. Execute emergency unstake after 5 days
        env.ledger().set_timestamp(now + 5 * 86400);
        let record = client.emergency_unstake(&staker, &xlm, &1_000_000);

        // 6. Verify penalty was applied
        assert!(record.penalty_amount > 0, "Penalty should be deducted");
        assert!(record.amount_returned < record.amount_requested);
        assert!(record.penalty_bps_applied > 500); // Should be closer to start penalty

        // 7. Verify cooldown is active
        assert!(client.is_in_cooldown(&staker));

        // 8. Check history
        let history = client.get_emergency_unstake_history(&staker);
        assert_eq!(history.len(), 1);
    }

    // ----- Example 8: Yield Distribution Scheduling -----
    //
    // Demonstrates scheduling and processing yield distributions.

    #[test]
    fn example_yield_distribution() {
        let (env, client, _admin) = setup();
        let staker = Address::generate(&env);
        let xlm = symbol_short!("XLM");

        let now = env.ledger().timestamp();

        // Schedule a one-off distribution
        let schedule = client.schedule_distribution(
            &staker,
            &xlm,
            &50_000,       // amount
            &(now + 86400), // due in 1 day
            &0,             // 0 = one-off
        );
        assert_eq!(schedule.amount, 50_000);
        assert!(!schedule.executed);

        // Not yet due — process returns 0
        let distributed = client.process_distribution(&staker, &xlm);
        assert_eq!(distributed, 0);

        // Advance time past due
        env.ledger().set_timestamp(now + 86401);

        // Now due — process returns the amount
        let distributed = client.process_distribution(&staker, &xlm);
        assert_eq!(distributed, 50_000);
    }
}

// ===========================================================================
// Rebalancing Contract Examples
// ===========================================================================

#[cfg(test)]
mod rebalancing_examples {
    use super::*;
    use astraport_rebalancing::{
        RebalancingContract, RebalancingContractClient, RebalanceInterval,
        TargetAllocation, CurrentHoldings,
    };

    fn setup() -> (Env, RebalancingContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, RebalancingContract);
        let client = RebalancingContractClient::new(&env, &contract_id);
        let _ = client.initialize();
        let owner = Address::generate(&env);
        (env, client, owner)
    }

    fn weights(env: &Env, entries: &[(Symbol, u32)]) -> soroban_sdk::Map<Symbol, u32> {
        let mut result = soroban_sdk::Map::new(env);
        for (asset, weight) in entries.iter() {
            result.set(asset.clone(), *weight);
        }
        result
    }

    // ----- Example 9: Complete Rebalancing Workflow -----
    //
    // Demonstrates the full portfolio setup → rebalance → history flow.

    #[test]
    fn example_complete_rebalancing_workflow() {
        let (env, client, owner) = setup();
        let portfolio = symbol_short!("DEFI_PORTFOLIO");

        // 1. Define target allocation: 40% USDC, 30% XLM, 30% BTC
        let target = TargetAllocation {
            allocations: weights(&env, &[
                (symbol_short!("USDC"), 4_000),
                (symbol_short!("XLM"), 3_000),
                (symbol_short!("BTC"), 3_000),
            ]),
        };

        // 2. Set target allocation
        let result = client.try_set_target_allocation(&owner, &portfolio, &target);
        assert!(result.is_ok());

        // 3. Record current holdings: 50% USDC, 20% XLM, 30% BTC
        let current = CurrentHoldings {
            allocations: weights(&env, &[
                (symbol_short!("USDC"), 5_000),
                (symbol_short!("XLM"), 2_000),
                (symbol_short!("BTC"), 3_000),
            ]),
        };
        let _ = client.try_set_current_holdings(&owner, &portfolio, &current);

        // 4. Execute rebalance
        let result = client.try_rebalance(&owner, &portfolio);
        let rebalance = result.unwrap().unwrap();

        // 5. Verify adjustments: USDC is overweight (sell), XLM is underweight (buy)
        assert!(rebalance.adjustments.len() > 0);

        // 6. Check execution history
        let history = client.get_execution_history(&portfolio);
        assert_eq!(history.len(), 1);
        assert_eq!(history.get(0).unwrap().details, symbol_short!("manual"));
    }

    // ----- Example 10: Access Control -----
    //
    // Demonstrates that only the owner can modify portfolio settings.

    #[test]
    fn example_access_control() {
        let (env, client, owner) = setup();
        let portfolio = symbol_short!("MY_PORTFOLIO");
        let non_owner = Address::generate(&env);

        // Owner sets allocation
        let target = TargetAllocation {
            allocations: weights(&env, &[(symbol_short!("USDC"), 10_000)]),
        };
        let _ = client.try_set_target_allocation(&owner, &portfolio, &target);

        // Non-owner tries to change allocation — should fail
        let result = client.try_set_target_allocation(&non_owner, &portfolio, &target);
        assert_eq!(result, Err(Ok(astraport_rebalancing::RebalancingError::Unauthorized)));

        // Non-owner tries to rebalance — should fail
        let result = client.try_rebalance(&non_owner, &portfolio);
        assert_eq!(result, Err(Ok(astraport_rebalancing::RebalancingError::Unauthorized)));
    }

    // ----- Example 11: Scheduled Rebalancing -----
    //
    // Demonstrates setting up and executing scheduled rebalancing.

    #[test]
    fn example_scheduled_rebalancing() {
        let (env, client, owner) = setup();
        let portfolio = symbol_short!("SCHED_PORT");

        // 1. Set target and current holdings (must match)
        let alloc = weights(&env, &[(symbol_short!("USDC"), 10_000)]);
        let _ = client.try_set_target_allocation(
            &owner,
            &portfolio,
            &TargetAllocation { allocations: alloc.clone() },
        );
        let _ = client.try_set_current_holdings(
            &owner,
            &portfolio,
            &CurrentHoldings { allocations: alloc },
        );

        // 2. Set daily rebalancing schedule
        let result = client.try_set_schedule(&owner, &portfolio, &RebalanceInterval::Daily);
        assert_eq!(result.unwrap(), Ok(symbol_short!("ok")));

        // 3. Check schedule — should not be due yet
        let result = client.try_check_exec_sched_rebalance(&portfolio);
        assert_eq!(result.unwrap(), Ok(symbol_short!("not_due")));

        // 4. Advance time by 1 day
        env.ledger().set_timestamp(env.ledger().timestamp() + 86_400);

        // 5. Now it should execute
        let result = client.try_check_exec_sched_rebalance(&portfolio);
        assert_eq!(result.unwrap(), Ok(symbol_short!("done")));
    }

    // ----- Example 12: Validation Error Handling -----
    //
    // Demonstrates rejection of invalid allocations.

    #[test]
    fn example_invalid_allocation_error() {
        let (env, client, owner) = setup();
        let portfolio = symbol_short!("INVALID_PORT");

        // Allocation that doesn't sum to 10,000 bps
        let invalid = TargetAllocation {
            allocations: weights(&env, &[
                (symbol_short!("USDC"), 4_000),
                (symbol_short!("XLM"), 3_000),
            ]),
        };

        let result = client.try_set_target_allocation(&owner, &portfolio, &invalid);
        assert_eq!(result, Err(Ok(astraport_rebalancing::RebalancingError::InvalidAllocation)));
    }
}

// ===========================================================================
// Events Contract Examples
// ===========================================================================

#[cfg(test)]
mod events_examples {
    use super::*;
    use astraport_events::EventsContract;
    use astraport_events::EventsContractClient;
    use astraport_events::{AITrigger, EventType};

    fn setup() -> (Env, EventsContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, EventsContract);
        let client = EventsContractClient::new(&env, &contract_id);
        let _ = client.initialize();
        (env, client)
    }

    // ----- Example 13: Event Subscription -----
    //
    // Demonstrates subscribing to portfolio events.

    #[test]
    fn example_event_subscription() {
        let (env, client) = setup();
        let portfolio = symbol_short!("PORTFOLIO1");
        let subscriber = Address::generate(&env);

        // Subscribe to events
        let result = client.try_subscribe(&portfolio, &subscriber);
        assert!(result.is_ok());

        // Get metrics
        let metrics = client.get_metrics();
        assert_eq!(metrics.total_analyses, 0);
    }

    // ----- Example 14: AI Trigger Setup -----
    //
    // Demonstrates creating and managing AI triggers.

    #[test]
    fn example_ai_trigger_management() {
        let (env, client) = setup();
        let owner = Address::generate(&env);
        let ai_endpoint = Address::generate(&env);

        // Create a trigger for portfolio rebalance events
        let trigger = AITrigger {
            trigger_id: symbol_short!("REBAL_ALERT"),
            name: symbol_short!("RebalanceAlert"),
            event_types: soroban_sdk::Vec::from_array(
                &env,
                &[EventType::PortfolioRebalance as u32],
            ),
            threshold: None,
            operator: None,
            ai_service_endpoint: ai_endpoint,
            timeout: 5000,
            is_active: true,
            owner: owner.clone(),
        };

        let result = client.try_add_trigger(&trigger);
        assert!(result.is_ok());

        // Verify trigger was added
        let triggers = client.get_all_triggers();
        assert_eq!(triggers.len(), 1);

        // Remove trigger
        let result = client.try_remove_trigger(&trigger.trigger_id, &owner);
        assert!(result.is_ok());
    }
}

// ===========================================================================
// Audit Contract Examples
// ===========================================================================

#[cfg(test)]
mod audit_examples {
    use super::*;
    use astraport_audit::AuditContract;
    use astraport_audit::AuditContractClient;
    use astraport_audit::log_query::LogQuery;
    use astraport_audit::records::{permissions, AuditEventType, StateSnapshot};

    fn setup() -> (Env, AuditContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, AuditContract);
        let client = AuditContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let _ = client.initialize(&admin);
        (env, client, admin)
    }

    fn snapshot(env: &Env, key: Symbol, value: i128) -> StateSnapshot {
        let mut s = StateSnapshot::empty(env);
        s.push(key, value);
        s
    }

    // ----- Example 15: Audit Logging and Querying -----
    //
    // Demonstrates logging events and querying them.

    #[test]
    fn example_audit_logging_and_querying() {
        let (env, client, _admin) = setup();
        let staker = Address::generate(&env);
        let portfolio = symbol_short!("XLM");

        // Log a stake event
        let seq1 = client.log_event(
            &staker,
            &AuditEventType::Stake,
            &portfolio,
            &permissions::STAKER,
            &StateSnapshot::empty(&env),
            &snapshot(&env, portfolio, 1_000_000),
            &symbol_short!("ok"),
            &soroban_sdk::String::from_str(&env, "Initial stake"),
        );
        assert_eq!(seq1, 1);

        // Log an unstake event
        let seq2 = client.log_event(
            &staker,
            &AuditEventType::Unstake,
            &portfolio,
            &permissions::STAKER,
            &snapshot(&env, portfolio, 1_000_000),
            &snapshot(&env, portfolio, 500_000),
            &symbol_short!("ok"),
            &soroban_sdk::String::from_str(&env, "Partial unstake"),
        );
        assert_eq!(seq2, 2);

        // Query all events
        let all = client.query(&LogQuery::new(10));
        assert_eq!(all.len(), 2);

        // Query by event type
        let stakes = client.query(&LogQuery::new(10).event_type(AuditEventType::Stake));
        assert_eq!(stakes.len(), 1);
        assert_eq!(stakes.get(0).unwrap().event_type, AuditEventType::Stake);

        // Query by portfolio
        let xlm_events = client.query(&LogQuery::new(10).portfolio(portfolio));
        assert_eq!(xlm_events.len(), 2);
    }

    // ----- Example 16: Integrity Verification -----
    //
    // Demonstrates chain-hash integrity checking.

    #[test]
    fn example_integrity_verification() {
        let (env, client, _admin) = setup();
        let staker = Address::generate(&env);
        let portfolio = symbol_short!("XLM");

        // Log some events
        for i in 0..5 {
            let _ = client.log_event(
                &staker,
                &AuditEventType::Stake,
                &portfolio,
                &permissions::STAKER,
                &StateSnapshot::empty(&env),
                &StateSnapshot::empty(&env),
                &symbol_short!("ok"),
                &soroban_sdk::String::from_str(&env, &format!("Event {}", i)),
            );
        }

        // Get the chain head
        let head = client.integrity_head();

        // Verify integrity matches
        assert!(client.verify_integrity(&head));

        // Full recompute should also pass
        assert!(client.full_recompute_integrity());

        // Bogus hash should fail
        let bogus = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);
        assert!(!client.verify_integrity(&bogus));
    }

    // ----- Example 17: Export -----
    //
    // Demonstrates JSON-Lines and CSV export.

    #[test]
    fn example_export() {
        let (env, client, _admin) = setup();
        let staker = Address::generate(&env);
        let portfolio = symbol_short!("XLM");

        let _ = client.log_event(
            &staker,
            &AuditEventType::Stake,
            &portfolio,
            &permissions::STAKER,
            &StateSnapshot::empty(&env),
            &snapshot(&env, portfolio, 100),
            &symbol_short!("ok"),
            &soroban_sdk::String::from_str(&env, "Test event"),
        );

        // Export as JSON-Lines
        let jsonl = client.export_jsonl(&LogQuery::new(10));
        assert_eq!(jsonl.len(), 1);
        let row = jsonl.get(0).unwrap().to_string();
        assert!(row.contains("\"seq\":1"));
        assert!(row.contains("\"event_type\":\"Stake\""));

        // Export as CSV
        let csv = client.export_csv(&LogQuery::new(10));
        assert_eq!(csv.len(), 2); // header + 1 row
        assert!(csv.get(0).unwrap().to_string().contains("seq,timestamp"));
        assert!(csv.get(1).unwrap().to_string().contains("Stake"));
    }
}
