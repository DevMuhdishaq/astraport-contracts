//! Unit and contract-level tests for the staking contract and yield engine.
//!
//! Tests are grouped into:
//! - original contract smoke tests (`initialize`, `get_balance`);
//! - authentication and balance tests;
//! - yield engine tests (accrual, rate changes, history, projections,
//!   distributions);
//! - emergency unstake tests.

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{symbol_short, Address, Env};

use crate::emergency::PenaltyDecayFunction;
use crate::fixed_point::{SCALE, SECONDS_PER_DAY, SECONDS_PER_YEAR};
use crate::records::{CompoundingMode, YieldDataKey};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup() -> (Env, StakingContractClient<'static>) {
    let env = Env::default();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    (env, client)
}

fn setup_with_admin() -> (Env, StakingContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

/// Assert `a` and `b` are within `tol`.
fn approx(a: i128, b: i128, tol: i128) {
    let diff = (a - b).abs();
    assert!(
        diff <= tol,
        "expected {} ~= {} within {}, diff {}",
        a,
        b,
        tol,
        diff
    );
}

// ---------------------------------------------------------------------------
// Smoke tests
// ---------------------------------------------------------------------------

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
fn test_get_balance_initial() {
    let env = Env::default();
    let staker = Address::generate(&env);
    let result = StakingContract::get_balance(env, staker);
    assert_eq!(result, 0);
}

#[test]
fn test_stake_and_unstake() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup();
    let staker = Address::generate(&env);

    assert_eq!(client.stake(&staker, &100), symbol_short!("done"));
    assert_eq!(client.get_balance(&staker), 100);

    assert_eq!(client.stake(&staker, &50), symbol_short!("done"));
    assert_eq!(client.get_balance(&staker), 150);

    assert_eq!(client.unstake(&staker, &75), symbol_short!("done"));
    assert_eq!(client.get_balance(&staker), 75);

    assert_eq!(client.unstake(&staker, &75), symbol_short!("done"));
    assert_eq!(client.get_balance(&staker), 0);
}

#[test]
#[should_panic(expected = "InvalidStakeAmount")]
fn test_stake_zero() {
    let (env, client) = setup();
    env.mock_all_auths();
    let staker = Address::generate(&env);
    client.stake(&staker, &0);
}

#[test]
#[should_panic(expected = "InsufficientBalance")]
fn test_unstake_more_than_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup();
    let staker = Address::generate(&env);
    client.stake(&staker, &100);
    client.unstake(&staker, &150);
}

// ---------------------------------------------------------------------------
// Authentication tests
// ---------------------------------------------------------------------------

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
    // No mock_auths — require_auth will fail.
    client.stake(&staker, &1_000);
}

#[test]
fn test_set_alert_threshold_requires_admin_auth() {
    let (env, client, admin) = setup_with_admin();
    client.set_alert_threshold(&admin, &10_000);
}

#[test]
#[should_panic(expected = "caller is not admin")]
fn test_set_alert_threshold_non_admin_fails() {
    let (env, client, _admin) = setup_with_admin();
    let non_admin = Address::generate(&env);
    client.set_alert_threshold(&non_admin, &10_000);
}

// ---------------------------------------------------------------------------
// Yield engine contract-level tests
// ---------------------------------------------------------------------------

#[test]
fn apr_to_apy_via_contract_matches_formula() {
    let (_env, client) = setup();
    let apy = client.apr_to_apy(&(SCALE / 20), &CompoundingMode::Daily);
    approx(apy, 51_267_496_505_408_400, 100_000_000_000);

    let apy_c = client.apr_to_apy(&(SCALE / 20), &CompoundingMode::Continuous);
    approx(apy_c, 51_271_096_376_024_040, 100_000_000_000);
}

#[test]
fn accrual_over_one_year_daily() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.open_yield_position(&staker, &asset, &SCALE, &(SCALE / 20), &CompoundingMode::Daily);
    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    let record = client.accrue_yield(&staker, &asset);
    approx(record.accrued_yield, 51_267_496_505_408_400, 100_000_000_000);
}

#[test]
fn apy_apr_roundtrip_via_contract() {
    let (_env, client) = setup();
    let apr = SCALE / 10;
    let apy = client.apr_to_apy(&apr, &CompoundingMode::Daily);
    let back = client.apy_to_apr(&apy, &CompoundingMode::Daily);
    approx(back, apr, 100_000_000_000_000);
}

#[test]
fn thirty_day_projection_within_one_percent() {
    let (_env, client) = setup();
    let horizon = 30 * SECONDS_PER_DAY;
    let proj = client.project_yield(&SCALE, &(SCALE / 10), &CompoundingMode::Continuous, &horizon);
    let expected = 8_253_048_640_000_000i128;
    let diff = (proj.projected_yield - expected).abs();
    assert!(diff <= expected / 100, "projection off >1%: {} vs {}", proj.projected_yield, expected);
    assert_eq!(proj.projected_balance, SCALE + proj.projected_yield);
}

#[test]
fn accrue_claim_and_reclaim_resets_unclaimed_yield() {
    let (env, client) = setup();
    env.mock_all_auths();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.open_yield_position(&staker, &asset, &SCALE, &(SCALE / 10), &CompoundingMode::Daily);

    env.ledger().set_timestamp(30 * SECONDS_PER_DAY);
    let accrued = client.accrue_yield(&staker, &asset).accrued_yield;
    assert!(accrued > 0);

    assert_eq!(client.claim_yield(&staker, &asset), accrued);
    assert_eq!(client.current_yield(&staker, &asset), 0);
    assert_eq!(client.claim_yield(&staker, &asset), 0);
}

// ---------------------------------------------------------------------------
// Emergency unstake — configuration tests
// ---------------------------------------------------------------------------

#[test]
fn configure_emergency_unstake_stores_config() {
    let (env, client, admin) = setup_with_admin();
    let treasury = Address::generate(&env);

    client.configure_emergency_unstake(
        &admin,
        &3_000,                          // 30% start penalty
        &500,                            // 5% end penalty
        &PenaltyDecayFunction::Linear,
        &(7 * 24 * 3600u64),             // 7-day cooldown
        &treasury,
        &true,
    );

    let cfg = client.get_emergency_config().unwrap();
    assert_eq!(cfg.penalty_start_bps, 3_000);
    assert_eq!(cfg.penalty_end_bps, 500);
    assert!(cfg.enabled);
    assert_eq!(cfg.cooldown_seconds, 7 * 24 * 3600);
}

#[test]
#[should_panic(expected = "caller is not admin")]
fn configure_emergency_unstake_requires_admin() {
    let (env, client, _admin) = setup_with_admin();
    let non_admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    client.configure_emergency_unstake(
        &non_admin,
        &3_000,
        &500,
        &PenaltyDecayFunction::Linear,
        &86_400u64,
        &treasury,
        &true,
    );
}

// ---------------------------------------------------------------------------
// Emergency unstake — core flow tests
// ---------------------------------------------------------------------------

/// Sets up the contract with an emergency-unstake config, a staker's stake,
/// and a lock position. Returns (env, client, admin, staker, treasury).
fn setup_emergency(
    lock_start_ts: u64,
    unlock_ts: u64,
    stake_amount: i128,
) -> (Env, StakingContractClient<'static>, Address, Address, Address) {
    let (env, client, admin) = setup_with_admin();
    env.ledger().set_timestamp(lock_start_ts);

    let treasury = Address::generate(&env);
    let staker = Address::generate(&env);

    // Configure emergency unstake: 30% → 5% linear decay, 1-day cooldown.
    client.configure_emergency_unstake(
        &admin,
        &3_000,
        &500,
        &PenaltyDecayFunction::Linear,
        &(24 * 3600u64),
        &treasury,
        &true,
    );

    // Stake.
    client.stake(&staker, &stake_amount);

    // Register lock position.
    client.set_lock_position(&admin, &staker, &lock_start_ts, &unlock_ts, &stake_amount);

    (env, client, admin, staker, treasury)
}

#[test]
fn emergency_unstake_at_start_applies_max_penalty() {
    let lock_start = 1_000_000u64;
    let total_lock = 30u64 * 24 * 3600;
    let unlock = lock_start + total_lock;

    let (env, client, _admin, staker, _treasury) =
        setup_emergency(lock_start, unlock, 1_000_000);

    // At t = lock_start (elapsed = 0) → start penalty = 30% (3000 bps).
    env.ledger().set_timestamp(lock_start);
    let record = client.emergency_unstake(&staker, &1_000_000);

    assert_eq!(record.penalty_bps_applied, 3_000);
    assert_eq!(record.penalty_amount, 300_000);    // 30% of 1_000_000
    assert_eq!(record.amount_returned, 700_000);   // 70% back to staker
    assert_eq!(record.amount_requested, 1_000_000);
    assert!(!record.is_partial);
}

#[test]
fn emergency_unstake_at_end_applies_min_penalty() {
    let lock_start = 1_000_000u64;
    let total_lock = 30u64 * 24 * 3600;
    let unlock = lock_start + total_lock;

    let (env, client, _admin, staker, _treasury) =
        setup_emergency(lock_start, unlock, 1_000_000);

    // At unlock (elapsed == total) → end penalty = 5% (500 bps).
    env.ledger().set_timestamp(unlock);
    let record = client.emergency_unstake(&staker, &1_000_000);

    assert_eq!(record.penalty_bps_applied, 500);
    assert_eq!(record.penalty_amount, 50_000);     // 5% of 1_000_000
    assert_eq!(record.amount_returned, 950_000);
}

#[test]
fn emergency_unstake_at_midpoint_applies_mid_penalty() {
    let lock_start = 1_000_000u64;
    let total_lock = 30u64 * 24 * 3600;
    let unlock = lock_start + total_lock;

    let (env, client, _admin, staker, _treasury) =
        setup_emergency(lock_start, unlock, 2_000_000);

    // At midpoint linear decay: penalty ~= (3000+500)/2 = 1750 bps.
    env.ledger().set_timestamp(lock_start + total_lock / 2);
    let record = client.emergency_unstake(&staker, &2_000_000);

    let expected_bps = 1_750i128;
    let diff = (record.penalty_bps_applied - expected_bps).abs();
    assert!(diff <= 5, "mid-point penalty {} != expected ~{}", record.penalty_bps_applied, expected_bps);
    assert_eq!(record.amount_returned + record.penalty_amount, 2_000_000);
}

#[test]
fn emergency_unstake_partial_reduces_balance_correctly() {
    let lock_start = 0u64;
    let total_lock = 30u64 * 24 * 3600;
    let unlock = lock_start + total_lock;

    let (env, client, _admin, staker, _treasury) =
        setup_emergency(lock_start, unlock, 1_000_000);

    // Partially unstake half.
    env.ledger().set_timestamp(lock_start);
    let record = client.emergency_unstake(&staker, &500_000);

    assert!(record.is_partial, "should be marked partial");
    assert_eq!(record.amount_requested, 500_000);
    // Balance reduced by full gross amount (500_000), not by net.
    assert_eq!(client.get_balance(&staker), 500_000);
}

#[test]
fn emergency_unstake_updates_history() {
    let lock_start = 0u64;
    let total_lock = 30u64 * 24 * 3600;
    let unlock = lock_start + total_lock;

    let (env, client, _admin, staker, _treasury) =
        setup_emergency(lock_start, unlock, 1_000_000);

    env.ledger().set_timestamp(lock_start + total_lock / 4);
    client.emergency_unstake(&staker, &200_000);
    env.ledger().set_timestamp(lock_start + total_lock * 2);  // past cooldown
    client.emergency_unstake(&staker, &100_000);

    let history = client.get_emergency_unstake_history(&staker);
    assert_eq!(history.len(), 2, "expected two emergency unstake records");
    assert_eq!(history.get(0).unwrap().amount_requested, 200_000);
    assert_eq!(history.get(1).unwrap().amount_requested, 100_000);
}

#[test]
fn emergency_unstake_activates_cooldown() {
    let lock_start = 0u64;
    let total_lock = 30u64 * 24 * 3600;
    let unlock = lock_start + total_lock;
    let cooldown = 24 * 3600u64;

    let (env, client, _admin, staker, _treasury) =
        setup_emergency(lock_start, unlock, 1_000_000);

    env.ledger().set_timestamp(lock_start);
    client.emergency_unstake(&staker, &100_000);

    // Cooldown should be active immediately after.
    assert!(client.is_in_cooldown(&staker));

    // End should be set correctly.
    let cooldown_end = client.get_cooldown_end(&staker);
    assert_eq!(cooldown_end, lock_start + cooldown);
}

#[test]
#[should_panic(expected = "CooldownActive")]
fn emergency_unstake_fails_during_cooldown() {
    let lock_start = 0u64;
    let total_lock = 30u64 * 24 * 3600;
    let unlock = lock_start + total_lock;

    let (env, client, _admin, staker, _treasury) =
        setup_emergency(lock_start, unlock, 1_000_000);

    env.ledger().set_timestamp(lock_start);
    client.emergency_unstake(&staker, &100_000);

    // Still within cooldown — this must panic.
    env.ledger().set_timestamp(lock_start + 3600); // only 1 hour later, cooldown = 1 day
    client.emergency_unstake(&staker, &100_000);
}

#[test]
fn cooldown_expires_and_second_emergency_unstake_succeeds() {
    let lock_start = 0u64;
    let total_lock = 30u64 * 24 * 3600;
    let unlock = lock_start + total_lock;
    let cooldown = 24 * 3600u64;

    let (env, client, _admin, staker, _treasury) =
        setup_emergency(lock_start, unlock, 1_000_000);

    env.ledger().set_timestamp(lock_start);
    client.emergency_unstake(&staker, &100_000);

    // After cooldown, another emergency unstake succeeds.
    env.ledger().set_timestamp(lock_start + cooldown + 1);
    assert!(!client.is_in_cooldown(&staker));
    let record = client.emergency_unstake(&staker, &100_000);
    assert_eq!(record.amount_requested, 100_000);
}

// ---------------------------------------------------------------------------
// Emergency unstake — exponential decay
// ---------------------------------------------------------------------------

#[test]
fn emergency_unstake_exponential_midpoint_lower_than_linear() {
    let lock_start = 0u64;
    let total_lock = 30u64 * 24 * 3600;
    let unlock = lock_start + total_lock;
    let stake = 1_000_000i128;

    // Linear config.
    let (env_lin, client_lin, admin_lin) = setup_with_admin();
    env_lin.mock_all_auths();
    env_lin.ledger().set_timestamp(lock_start);
    let treasury_lin = Address::generate(&env_lin);
    let staker_lin = Address::generate(&env_lin);
    client_lin.configure_emergency_unstake(
        &admin_lin, &4_000, &400, &PenaltyDecayFunction::Linear,
        &0u64, &treasury_lin, &true,
    );
    client_lin.stake(&staker_lin, &stake);
    client_lin.set_lock_position(&admin_lin, &staker_lin, &lock_start, &unlock, &stake);
    env_lin.ledger().set_timestamp(lock_start + total_lock / 2);
    let rec_lin = client_lin.emergency_unstake(&staker_lin, &stake);

    // Exponential config.
    let (env_exp, client_exp, admin_exp) = setup_with_admin();
    env_exp.mock_all_auths();
    env_exp.ledger().set_timestamp(lock_start);
    let treasury_exp = Address::generate(&env_exp);
    let staker_exp = Address::generate(&env_exp);
    client_exp.configure_emergency_unstake(
        &admin_exp, &4_000, &400, &PenaltyDecayFunction::Exponential,
        &0u64, &treasury_exp, &true,
    );
    client_exp.stake(&staker_exp, &stake);
    client_exp.set_lock_position(&admin_exp, &staker_exp, &lock_start, &unlock, &stake);
    env_exp.ledger().set_timestamp(lock_start + total_lock / 2);
    let rec_exp = client_exp.emergency_unstake(&staker_exp, &stake);

    assert!(
        rec_exp.penalty_bps_applied < rec_lin.penalty_bps_applied,
        "exponential mid penalty {} should be < linear mid penalty {}",
        rec_exp.penalty_bps_applied,
        rec_lin.penalty_bps_applied,
    );
}

// ---------------------------------------------------------------------------
// Emergency unstake — error paths
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "EmergencyUnstakeConfig not initialized")]
fn emergency_unstake_without_config_panics() {
    let (env, client, _admin) = setup_with_admin();
    env.mock_all_auths();
    let staker = Address::generate(&env);
    client.stake(&staker, &1_000_000);
    // No configure_emergency_unstake call → should panic.
    client.emergency_unstake(&staker, &500_000);
}

#[test]
#[should_panic(expected = "EmergencyUnstakeDisabled")]
fn emergency_unstake_when_disabled_panics() {
    let (env, client, admin) = setup_with_admin();
    let treasury = Address::generate(&env);
    let staker = Address::generate(&env);

    client.configure_emergency_unstake(
        &admin, &3_000, &500, &PenaltyDecayFunction::Linear,
        &86_400u64, &treasury, &false, // disabled
    );
    client.stake(&staker, &1_000_000);
    client.emergency_unstake(&staker, &500_000);
}

#[test]
#[should_panic(expected = "InsufficientBalanceForEmergencyUnstake")]
fn emergency_unstake_more_than_balance_panics() {
    let lock_start = 0u64;
    let total_lock = 30u64 * 24 * 3600;
    let unlock = lock_start + total_lock;

    let (env, client, _admin, staker, _treasury) =
        setup_emergency(lock_start, unlock, 500_000);

    env.ledger().set_timestamp(lock_start);
    client.emergency_unstake(&staker, &600_000); // more than staked
}

// ---------------------------------------------------------------------------
// Preview penalty (pure query, no state change)
// ---------------------------------------------------------------------------

#[test]
fn preview_penalty_matches_actual_applied_penalty() {
    let lock_start = 0u64;
    let total_lock = 30u64 * 24 * 3600;
    let unlock = lock_start + total_lock;

    let (env, client, _admin, staker, _treasury) =
        setup_emergency(lock_start, unlock, 1_000_000);

    let query_ts = lock_start + total_lock / 3;
    env.ledger().set_timestamp(query_ts);

    let preview_bps = client.preview_emergency_penalty(&lock_start, &unlock).unwrap();

    // Actually perform the emergency unstake and compare.
    let record = client.emergency_unstake(&staker, &1_000_000);
    assert_eq!(
        preview_bps, record.penalty_bps_applied,
        "preview {} should match applied {}",
        preview_bps,
        record.penalty_bps_applied
    );
}
