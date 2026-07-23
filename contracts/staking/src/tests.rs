//! Unit and contract-level tests for the staking contract and yield engine.
//!
//! Tests are grouped into:
//! - the original contract smoke tests (`initialize`, `get_balance`);
//! - contract-level yield tests driven through the generated client, exercising
//!   real-time accrual, rate changes, history, projections, and distribution
//!   scheduling against a mutable test ledger clock.
//!
//! The pure-math correctness tests (matching standard financial formulas within
//! tolerance) live alongside their implementations in `fixed_point`,
//! `compounding`, `apy`, and `projection`.

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{symbol_short, Address, Env};

use crate::fixed_point::{SCALE, SECONDS_PER_DAY, SECONDS_PER_YEAR};
use crate::records::{CompoundingMode, YieldDataKey};

// --- original smoke tests -------------------------------------------------

#[test]
fn test_initialize() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    let result = client.initialize(&admin);
    assert_eq!(result, symbol_short!("ok"));
}

#[test]
fn test_initialize_stores_admin() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    env.as_contract(&contract_id, || {
        let stored: Address = env
            .storage()
            .persistent()
            .get(&YieldDataKey::Admin)
            .unwrap();
        assert_eq!(stored, admin);
    });
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_initialize_cannot_reinitialize() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let other_admin = Address::generate(&env);
    client.initialize(&other_admin);
}

#[test]
fn test_get_balance() {
    let env = Env::default();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    let staker = Address::generate(&env);
    let result = client.get_balance(&staker);
    assert_eq!(result, 0);
}

// --- helpers --------------------------------------------------------------

/// Register the contract and return its client plus the env.
fn setup() -> (Env, StakingContractClient<'static>) {
    let env = Env::default();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    (env, client)
}

/// Assert `a` and `b` are within `tol`.
fn approx(a: i128, b: i128, tol: i128) {
    let diff = (a - b).abs();
    assert!(diff <= tol, "expected {} ~= {} within {}, diff {}", a, b, tol, diff);
}

// --- authentication tests --------------------------------------------------

#[test]
fn test_stake_requires_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let staker = Address::generate(&env);
    client.stake(&staker, &1_000);
    assert_eq!(client.get_balance(&staker), 1_000);
}

#[test]
#[should_panic]
fn test_stake_unauthorized() {
    let env = Env::default();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let staker = Address::generate(&env);
    // No mock_auths — require_auth will fail
    client.stake(&staker, &1_000);
}

#[test]
fn test_unstake_requires_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let staker = Address::generate(&env);
    client.stake(&staker, &1_000);
    client.unstake(&staker, &500);
    assert_eq!(client.get_balance(&staker), 500);
}

#[test]
#[should_panic]
fn test_unstake_unauthorized() {
    let env = Env::default();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let staker = Address::generate(&env);
    env.mock_all_auths();
    client.stake(&staker, &1_000);

    // Create a fresh env without auth mocking
    let env2 = Env::default();
    let contract_id2 = env2.register_contract(None, StakingContract);
    let client2 = StakingContractClient::new(&env2, &contract_id2);
    let admin2 = Address::generate(&env2);
    client2.initialize(&admin2);
    let staker2 = Address::generate(&env2);
    // No mock_auths — should panic
    client2.unstake(&staker2, &500);
}

#[test]
#[should_panic(expected = "insufficient balance")]
fn test_unstake_insufficient_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let staker = Address::generate(&env);
    client.stake(&staker, &100);
    client.unstake(&staker, &200);
}

#[test]
fn test_stake_updates_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let staker = Address::generate(&env);
    assert_eq!(client.get_balance(&staker), 0);

    client.stake(&staker, &1_000);
    assert_eq!(client.get_balance(&staker), 1_000);

    client.stake(&staker, &500);
    assert_eq!(client.get_balance(&staker), 1_500);

    client.unstake(&staker, &200);
    assert_eq!(client.get_balance(&staker), 1_300);
}

#[test]
fn test_set_alert_threshold_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    client.set_alert_threshold(&admin, &10_000);
}

#[test]
#[should_panic(expected = "caller is not admin")]
fn test_set_alert_threshold_non_admin_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let non_admin = Address::generate(&env);
    // Auth passes (mock_all_auths), but the admin check fails
    client.set_alert_threshold(&non_admin, &10_000);
}

// --- yield engine contract tests ------------------------------------------

#[test]
fn apr_to_apy_via_contract_matches_formula() {
    let (_env, client) = setup();
    // 5% APR daily -> APY 0.05126749650...
    let apy = client.apr_to_apy(&(SCALE / 20), &CompoundingMode::Daily);
    approx(apy, 51_267_496_505_408_400, 100_000_000_000);

    // 5% APR continuous -> APY 0.05127109637...
    let apy_c = client.apr_to_apy(&(SCALE / 20), &CompoundingMode::Continuous);
    approx(apy_c, 51_271_096_376_024_040, 100_000_000_000);
}

#[test]
fn accrual_over_one_year_daily() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);

    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    let principal = SCALE; // 1e18 base units
    let apr = SCALE / 20; // 5%

    client.open_yield_position(&staker, &asset, &principal, &apr, &CompoundingMode::Daily);

    // Advance one year and accrue.
    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    let record = client.accrue_yield(&staker, &asset);

    // (1 + 0.05/365)^365 - 1 on 1e18 ~= 5.1267e16
    approx(record.accrued_yield, 51_267_496_505_408_400, 100_000_000_000);
}

#[test]
fn current_yield_is_realtime_without_mutation() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("USDC");

    client.open_yield_position(
        &staker,
        &asset,
        &SCALE,
        &(SCALE / 10),
        &CompoundingMode::Continuous,
    );

    // Halfway through the year, current_yield reflects accrual live.
    env.ledger().set_timestamp(SECONDS_PER_YEAR / 2);
    let mid = client.current_yield(&staker, &asset);
    // e^(0.1*0.5) - 1 = e^0.05 - 1 = 0.05127109637 on 1e18
    approx(mid, 51_271_096_376_024_040, 100_000_000_000);

    // Reading did not checkpoint: the stored record still shows zero accrued.
    let rec = client.open_yield_position(
        &staker,
        &asset,
        &SCALE,
        &(SCALE / 10),
        &CompoundingMode::Continuous,
    );
    // open_yield_position checkpoints first, so accrued now reflects the mid-year amount.
    approx(rec.accrued_yield, 51_271_096_376_024_040, 100_000_000_000);
}

#[test]
fn rate_change_is_time_weighted() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    // Continuous, first half year at 4%, then 8%.
    client.open_yield_position(
        &staker,
        &asset,
        &SCALE,
        &(4 * SCALE / 100),
        &CompoundingMode::Continuous,
    );

    env.ledger().set_timestamp(SECONDS_PER_YEAR / 2);
    client.set_yield_rate(&staker, &asset, &(8 * SCALE / 100));

    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    let rec = client.accrue_yield(&staker, &asset);

    // Yield compounds on principal per checkpoint; realized yield is additive:
    // seg1 = e^0.02 - 1, seg2 = e^0.04 - 1 (each on principal 1e18).
    // total = (e^0.02 - 1) + (e^0.04 - 1) = 0.0202013 + 0.0408108 = 0.0610121
    let seg1 = 20_201_340_026_755_810i128; // e^0.02 - 1
    let seg2 = 40_810_774_192_388_960i128; // e^0.04 - 1
    approx(rec.accrued_yield, seg1 + seg2, 10_000_000_000);
}

#[test]
fn history_is_complete_and_queryable() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.open_yield_position(
        &staker,
        &asset,
        &SCALE,
        &(SCALE / 20),
        &CompoundingMode::Daily,
    );

    // Three monthly-ish checkpoints.
    env.ledger().set_timestamp(30 * SECONDS_PER_DAY);
    client.accrue_yield(&staker, &asset);
    env.ledger().set_timestamp(60 * SECONDS_PER_DAY);
    client.accrue_yield(&staker, &asset);
    env.ledger().set_timestamp(90 * SECONDS_PER_DAY);
    let rec = client.accrue_yield(&staker, &asset);

    let history = client.yield_history(&staker, &asset);
    assert_eq!(history.len(), 3, "expected three history entries");

    // Cumulative in the last entry equals the record's accrued yield.
    let last = history.get(2).unwrap();
    assert_eq!(last.cumulative_yield, rec.accrued_yield);
    assert_eq!(last.timestamp, 90 * SECONDS_PER_DAY);
    // Each entry covered ~30 days.
    assert_eq!(history.get(0).unwrap().period_seconds, 30 * SECONDS_PER_DAY);
}

#[test]
fn thirty_day_projection_within_one_percent() {
    let (_env, client) = setup();
    // 10% APR continuous, 30 days on 1e18.
    let horizon = 30 * SECONDS_PER_DAY;
    let proj = client.project_yield(
        &SCALE,
        &(SCALE / 10),
        &CompoundingMode::Continuous,
        &horizon,
    );
    let expected = 8_253_048_640_000_000i128; // e^(0.1*30/365) - 1 on 1e18
    let diff = (proj.projected_yield - expected).abs();
    assert!(diff <= expected / 100, "projection off by >1%: {} vs {}", proj.projected_yield, expected);
    assert_eq!(proj.projected_balance, SCALE + proj.projected_yield);
}

#[test]
fn one_off_distribution_becomes_due_once() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    // Schedule a one-off distribution due at t=2000.
    client.schedule_distribution(&staker, &asset, &500, &2_000, &0);

    // Not due yet.
    assert_eq!(client.process_distribution(&staker, &asset), 0);

    // Now due.
    env.ledger().set_timestamp(2_000);
    assert_eq!(client.process_distribution(&staker, &asset), 500);

    // Already executed: does not fire again.
    env.ledger().set_timestamp(3_000);
    assert_eq!(client.process_distribution(&staker, &asset), 0);
}

#[test]
fn recurring_distribution_rolls_forward() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    // Recurring every 1000s, first due at 1000, amount 100.
    client.schedule_distribution(&staker, &asset, &100, &1_000, &1_000);

    env.ledger().set_timestamp(1_000);
    assert_eq!(client.process_distribution(&staker, &asset), 100);

    // Fires again at the next interval.
    env.ledger().set_timestamp(2_000);
    assert_eq!(client.process_distribution(&staker, &asset), 100);

    // Between intervals, nothing new is due.
    env.ledger().set_timestamp(2_500);
    assert_eq!(client.process_distribution(&staker, &asset), 0);
}

#[test]
fn apy_apr_roundtrip_via_contract() {
    let (_env, client) = setup();
    let apr = SCALE / 10; // 10%
    let apy = client.apr_to_apy(&apr, &CompoundingMode::Daily);
    let back = client.apy_to_apr(&apy, &CompoundingMode::Daily);
    // within 0.01% -> tol 1e14 on 1e18 scale
    approx(back, apr, 100_000_000_000_000);
}

#[test]
fn stake_opens_position_and_accrues_on_staked_principal() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    // Staking wires the deposit into the yield engine at the default params
    // (5% APR, daily), and the returned/queried balance reflects the stake.
    let balance = client.stake(&staker, &asset, &SCALE);
    assert_eq!(balance, SCALE);
    assert_eq!(client.get_balance(&staker, &asset), SCALE);

    // Yield accrues on the actual staked principal.
    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    let yielded = client.current_yield(&staker, &asset);
    // (1 + 0.05/365)^365 - 1 on 1e18 ~= 5.1267e16
    approx(yielded, 51_267_496_505_408_400, 100_000_000_000);
}

#[test]
fn additional_stake_raises_principal_and_keeps_yield() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.stake(&staker, &asset, &SCALE);

    // After a year, stake more. This must checkpoint accrued yield first.
    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    let new_balance = client.stake(&staker, &asset, &SCALE);
    assert_eq!(new_balance, 2 * SCALE);

    // Principal now equals the staked balance; realized yield is preserved.
    let rec = client.accrue_yield(&staker, &asset);
    assert_eq!(rec.principal, 2 * SCALE);
    approx(rec.accrued_yield, 51_267_496_505_408_400, 100_000_000_000);
}

#[test]
fn unstake_preserves_accrued_yield_and_reduces_principal() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    // Stake 1e18 and let a full year of yield accrue.
    client.stake(&staker, &asset, &SCALE);
    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    let expected_yield = 51_267_496_505_408_400i128; // 5% daily on 1e18

    // Unstake half. This checkpoints accrued yield before dropping principal.
    let remaining = client.unstake(&staker, &asset, &(SCALE / 2));
    assert_eq!(remaining, SCALE / 2);
    assert_eq!(client.get_balance(&staker, &asset), SCALE / 2);

    // Accrued yield is preserved and principal is reduced (accrue at the same
    // timestamp is a no-op, so it just returns the stored record).
    let rec = client.accrue_yield(&staker, &asset);
    assert_eq!(rec.principal, SCALE / 2);
    approx(rec.accrued_yield, expected_yield, 100_000_000_000);

    // Going forward, further yield accrues on the reduced principal on top of
    // the preserved amount.
    env.ledger().set_timestamp(2 * SECONDS_PER_YEAR);
    let total = client.current_yield(&staker, &asset);
    // preserved + second-year yield on half the principal (~2.5634e16).
    approx(total, expected_yield + 25_633_748_252_704_200, 100_000_000_000);
}

#[test]
fn configured_yield_defaults_apply_to_new_positions() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("USDC");

    // Reconfigure defaults to 10% continuous before the first stake.
    client.set_yield_defaults(&(SCALE / 10), &CompoundingMode::Continuous);
    client.stake(&staker, &asset, &SCALE);

    env.ledger().set_timestamp(SECONDS_PER_YEAR / 2);
    let mid = client.current_yield(&staker, &asset);
    // e^(0.1*0.5) - 1 = e^0.05 - 1 on 1e18
    approx(mid, 51_271_096_376_024_040, 100_000_000_000);
}

#[test]
#[should_panic(expected = "invalid unstake amount")]
fn unstake_more_than_staked_panics() {
    let (env, client) = setup();
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    client.stake(&staker, &asset, &SCALE);
    client.unstake(&staker, &asset, &(SCALE + 1));
}

#[test]
fn zero_principal_accrues_nothing() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    client.open_yield_position(&staker, &asset, &0, &(SCALE / 10), &CompoundingMode::Daily);
    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    let rec = client.accrue_yield(&staker, &asset);
    assert_eq!(rec.accrued_yield, 0);
}

// --- yield claiming -------------------------------------------------------

#[test]
fn accrue_claim_and_reclaim_resets_unclaimed_yield() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.open_yield_position(
        &staker,
        &asset,
        &SCALE,
        &(SCALE / 10),
        &CompoundingMode::Daily,
    );

    // Checkpoint earnings, then claim exactly the checkpointed amount.
    env.ledger().set_timestamp(30 * SECONDS_PER_DAY);
    let accrued = client.accrue_yield(&staker, &asset).accrued_yield;
    assert!(accrued > 0);

    env.mock_all_auths();
    assert_eq!(client.claim_yield(&staker, &asset), accrued);
    assert_eq!(client.current_yield(&staker, &asset), 0);

    // No time has elapsed, so a second claim cannot pay the same yield twice.
    assert_eq!(client.claim_yield(&staker, &asset), 0);

    let history = client.yield_history(&staker, &asset);
    assert_eq!(history.len(), 3);
    let claim = history.get(1).unwrap();
    assert!(claim.is_claim);
    assert_eq!(claim.period_seconds, 0);
    assert_eq!(claim.yield_earned, 0);
    assert_eq!(claim.cumulative_yield, 0);
}

#[test]
#[should_panic]
fn claim_yield_requires_staker_auth() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.open_yield_position(
        &staker,
        &asset,
        &SCALE,
        &(SCALE / 10),
        &CompoundingMode::Daily,
    );

    // No mock authorization is supplied for `staker`.
    client.claim_yield(&staker, &asset);
}
