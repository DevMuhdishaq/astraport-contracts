//! Unit and contract-level tests for the multi-asset staking contract.
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
use crate::records::CompoundingMode;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup() -> (Env, StakingContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    (env, client)
}

fn setup_with_admin() -> (Env, StakingContractClient<'static>, Address) {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

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
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    assert_eq!(client.initialize(&admin), symbol_short!("ok"));
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_double_initialize_panics() {
    let (_env, client, admin) = setup_with_admin();
    client.initialize(&admin);
}

// ---------------------------------------------------------------------------
// Basic multi-asset stake / unstake / balance
// ---------------------------------------------------------------------------

#[test]
fn test_get_balance_initial() {
    let (env, client) = setup();
    let staker = Address::generate(&env);
    assert_eq!(client.get_balance(&staker, &symbol_short!("XLM")), 0);
}

#[test]
fn test_stake_and_unstake() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup();
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    assert_eq!(client.stake(&staker, &asset, &100), symbol_short!("ok"));
    assert_eq!(client.get_balance(&staker, &asset), 100);

    assert_eq!(client.stake(&staker, &asset, &50), symbol_short!("ok"));
    assert_eq!(client.get_balance(&staker, &asset), 150);

    assert_eq!(client.unstake(&staker, &asset, &75), symbol_short!("ok"));
    assert_eq!(client.get_balance(&staker, &asset), 75);

    assert_eq!(client.unstake(&staker, &asset, &75), symbol_short!("ok"));
    assert_eq!(client.get_balance(&staker, &asset), 0);
}

#[test]
fn test_stake_multiple_assets_independently() {
    let (env, client) = setup();
    env.mock_all_auths();
    let staker = Address::generate(&env);
    let xlm = symbol_short!("XLM");
    let usdc = symbol_short!("USDC");
    let btc = symbol_short!("BTC");

    client.stake(&staker, &xlm, &1_000);
    client.stake(&staker, &usdc, &2_000);
    client.stake(&staker, &btc, &500);

    assert_eq!(client.get_balance(&staker, &xlm), 1_000);
    assert_eq!(client.get_balance(&staker, &usdc), 2_000);
    assert_eq!(client.get_balance(&staker, &btc), 500);

    // Unstaking one asset does not affect others.
    client.unstake(&staker, &xlm, &1_000);
    assert_eq!(client.get_balance(&staker, &xlm), 0);
    assert_eq!(client.get_balance(&staker, &usdc), 2_000);
    assert_eq!(client.get_balance(&staker, &btc), 500);
}

#[test]
fn test_unstake_more_than_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let (_, client) = setup();
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    client.stake(&staker, &asset, &100);
    // `unstake` returns `Result<Symbol, Error>`; soroban-sdk auto-generates
    // `try_unstake` which returns
    //   Result<
    //     Result<Symbol, soroban_sdk::ConversionError>,
    //     Result<crate::Error, soroban_sdk::InvokeError>
    //   >
    // i.e. outer Err + inner Ok = contract returned its own Error variant.
    assert_eq!(
        client.try_unstake(&staker, &asset, &150),
        Err(Ok(crate::Error::InsufficientBalance)),
    );
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
    let asset = symbol_short!("XLM");
    client.stake(&staker, &asset, &1_000);
    assert_eq!(client.get_balance(&staker, &asset), 1_000);
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
    let asset = symbol_short!("XLM");
    // No mock_auths — require_auth will fail.
    client.stake(&staker, &asset, &1_000);
}

#[test]
fn test_set_alert_threshold_requires_admin_auth() {
    let (_env, client, admin) = setup_with_admin();
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
fn test_set_yield_defaults_applies_to_new_positions() {
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

// ---------------------------------------------------------------------------
// Unlock schedules
// ---------------------------------------------------------------------------

#[test]
fn accrue_claim_and_reclaim_resets_unclaimed_yield() {
    let (env, client) = setup();
    env.mock_all_auths();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("VESTED");

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
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.configure_emergency_unstake(
        &admin,
        &3_000,                          // 30% start penalty
        &500,                            // 5% end penalty
        &PenaltyDecayFunction::Linear,
        &(7 * 24 * 3600u64),             // 7-day cooldown
        &treasury,
        &true,
    );
    client.stake(&staker, &asset, &1_000);

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
    let asset = symbol_short!("XLM");
    client.stake(&staker, &asset, &stake_amount);

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
    let asset = symbol_short!("XLM");
    let record = client.emergency_unstake(&staker, &asset, &1_000_000);

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
    let asset = symbol_short!("XLM");
    let record = client.emergency_unstake(&staker, &asset, &1_000_000);

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
    let asset = symbol_short!("XLM");
    let record = client.emergency_unstake(&staker, &asset, &2_000_000);

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
    let asset = symbol_short!("XLM");
    let record = client.emergency_unstake(&staker, &asset, &500_000);

    assert!(record.is_partial, "should be marked partial");
    assert_eq!(record.amount_requested, 500_000);
    // Balance reduced by full gross amount (500_000), not by net.
    assert_eq!(client.get_balance(&staker, &asset), 500_000);
}

#[test]
fn emergency_unstake_updates_history() {
    let lock_start = 0u64;
    let total_lock = 30u64 * 24 * 3600;
    let unlock = lock_start + total_lock;

    let (env, client, _admin, staker, _treasury) =
        setup_emergency(lock_start, unlock, 1_000_000);

    env.ledger().set_timestamp(lock_start + total_lock / 4);
    let asset = symbol_short!("XLM");
    client.emergency_unstake(&staker, &asset, &200_000);
    env.ledger().set_timestamp(lock_start + total_lock * 2);  // past cooldown
    client.emergency_unstake(&staker, &asset, &100_000);

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
    let asset = symbol_short!("XLM");
    client.emergency_unstake(&staker, &asset, &100_000);

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
    let asset = symbol_short!("XLM");
    client.emergency_unstake(&staker, &asset, &100_000);

    // Still within cooldown — this must panic.
    env.ledger().set_timestamp(lock_start + 3600); // only 1 hour later, cooldown = 1 day
    client.emergency_unstake(&staker, &asset, &100_000);
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
    let asset = symbol_short!("XLM");
    client.emergency_unstake(&staker, &asset, &100_000);

    // After cooldown, another emergency unstake succeeds.
    env.ledger().set_timestamp(lock_start + cooldown + 1);
    assert!(!client.is_in_cooldown(&staker));
    let record = client.emergency_unstake(&staker, &asset, &100_000);
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
    let asset_lin = symbol_short!("XLM");
    client_lin.configure_emergency_unstake(
        &admin_lin, &4_000, &400, &PenaltyDecayFunction::Linear,
        &0u64, &treasury_lin, &true,
    );
    client_lin.stake(&staker_lin, &asset_lin, &stake);
    client_lin.set_lock_position(&admin_lin, &staker_lin, &lock_start, &unlock, &stake);
    env_lin.ledger().set_timestamp(lock_start + total_lock / 2);
    let rec_lin = client_lin.emergency_unstake(&staker_lin, &asset_lin, &stake);

    // Exponential config.
    let (env_exp, client_exp, admin_exp) = setup_with_admin();
    env_exp.mock_all_auths();
    env_exp.ledger().set_timestamp(lock_start);
    let treasury_exp = Address::generate(&env_exp);
    let staker_exp = Address::generate(&env_exp);
    let asset_exp = symbol_short!("XLM");
    client_exp.configure_emergency_unstake(
        &admin_exp, &4_000, &400, &PenaltyDecayFunction::Exponential,
        &0u64, &treasury_exp, &true,
    );
    client_exp.stake(&staker_exp, &asset_exp, &stake);
    client_exp.set_lock_position(&admin_exp, &staker_exp, &lock_start, &unlock, &stake);
    env_exp.ledger().set_timestamp(lock_start + total_lock / 2);
    let rec_exp = client_exp.emergency_unstake(&staker_exp, &asset_exp, &stake);

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
    let asset = symbol_short!("XLM");
    client.stake(&staker, &asset, &1_000_000);
    // No configure_emergency_unstake call → should panic.
    client.emergency_unstake(&staker, &asset, &500_000);
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
    let asset = symbol_short!("XLM");
    client.stake(&staker, &asset, &1_000_000);
    client.emergency_unstake(&staker, &asset, &500_000);
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
    let asset = symbol_short!("XLM");
    client.emergency_unstake(&staker, &asset, &600_000); // more than staked
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
    let asset = symbol_short!("XLM");
    let record = client.emergency_unstake(&staker, &asset, &1_000_000);
    assert_eq!(
        preview_bps, record.penalty_bps_applied,
        "preview {} should match applied {}",
        preview_bps,
        record.penalty_bps_applied
    );
}

#[test]
fn test_zero_principal_accrues_nothing() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    client.open_yield_position(&staker, &asset, &0, &(SCALE / 10), &CompoundingMode::Daily);
    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    assert_eq!(client.accrue_yield(&staker, &asset).accrued_yield, 0);
}

// ---------------------------------------------------------------------------
// Protocol-level totals: total_staked(asset) + staker_count()
// ---------------------------------------------------------------------------

#[test]
fn test_totals_initial_state() {
    let (env, _client) = setup();
    env.mock_all_auths();
    let xlm = symbol_short!("XLM");
    let usdc = symbol_short!("USDC");
    assert_eq!(_client.total_staked(&xlm), 0);
    assert_eq!(_client.total_staked(&usdc), 0);
    assert_eq!(_client.staker_count(), 0);
}

#[test]
fn test_total_staked_reflects_stake_and_unstake_one_staker() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    let staker = Address::generate(&env);
    let xlm = symbol_short!("XLM");

    client.stake(&staker, &xlm, &1_000);
    assert_eq!(client.total_staked(&xlm), 1_000);
    assert_eq!(client.staker_count(), 1);

    client.stake(&staker, &xlm, &500);
    assert_eq!(client.total_staked(&xlm), 1_500);
    assert_eq!(client.staker_count(), 1, "same staker, same asset: count stays at 1");

    // Partial unstake keeps both stakers count and total above zero.
    client.unstake(&staker, &xlm, &700);
    assert_eq!(client.total_staked(&xlm), 800);
    assert_eq!(client.staker_count(), 1);

    // Full exit drains total AND decrements staker count to zero.
    client.unstake(&staker, &xlm, &800);
    assert_eq!(client.total_staked(&xlm), 0);
    assert_eq!(client.staker_count(), 0);
}

#[test]
fn test_total_staked_sums_across_multiple_stakers() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let carol = Address::generate(&env);
    let xlm = symbol_short!("XLM");

    client.stake(&alice, &xlm, &1_000);
    client.stake(&bob, &xlm, &2_500);
    client.stake(&carol, &xlm, &750);

    assert_eq!(client.total_staked(&xlm), 1_000 + 2_500 + 750);
    assert_eq!(client.staker_count(), 3);

    // Cross-check: sum of balances equals total_staked.
    assert_eq!(
        client.get_balance(&alice, &xlm)
            .checked_add(client.get_balance(&bob, &xlm))
            .and_then(|s| s.checked_add(client.get_balance(&carol, &xlm)))
            .unwrap(),
        client.total_staked(&xlm),
    );
}

#[test]
fn test_totals_are_per_asset() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    let staker = Address::generate(&env);
    let xlm = symbol_short!("XLM");
    let usdc = symbol_short!("USDC");

    client.stake(&staker, &xlm, &1_000);
    client.stake(&staker, &usdc, &5_000);

    assert_eq!(client.total_staked(&xlm), 1_000);
    assert_eq!(client.total_staked(&usdc), 5_000);
    // Distinct staker (one address) with two active positions still counts as 1.
    assert_eq!(client.staker_count(), 1);

    // Full exit on XLM leaves USDC untouched.
    client.unstake(&staker, &xlm, &1_000);
    assert_eq!(client.total_staked(&xlm), 0);
    assert_eq!(client.total_staked(&usdc), 5_000);
    assert_eq!(client.staker_count(), 1, "still active in usdc");

    // Full exit on USDC brings staker count to zero.
    client.unstake(&staker, &usdc, &5_000);
    assert_eq!(client.total_staked(&usdc), 0);
    assert_eq!(client.staker_count(), 0);
}

#[test]
fn test_staker_count_increments_only_on_first_active_position() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);

    let s1 = Address::generate(&env);
    let s2 = Address::generate(&env);
    let s3 = Address::generate(&env);
    let xlm = symbol_short!("XLM");

    assert_eq!(client.staker_count(), 0);

    client.stake(&s1, &xlm, &100);
    assert_eq!(client.staker_count(), 1);

    client.stake(&s1, &xlm, &50); // same (staker, asset); no increment
    client.stake(&s2, &xlm, &100); // new staker
    assert_eq!(client.staker_count(), 2);

    client.stake(&s3, &xlm, &200); // new staker
    assert_eq!(client.staker_count(), 3);

    // Partial unstakes do NOT change staker_count.
    client.unstake(&s1, &xlm, &50);
    client.unstake(&s2, &xlm, &50);
    assert_eq!(client.staker_count(), 3);

    // Full exit of one staker decrements by exactly 1.
    client.unstake(&s1, &xlm, &100);
    assert_eq!(client.staker_count(), 2);

    // Full exit of a second staker.
    client.unstake(&s3, &xlm, &200);
    assert_eq!(client.staker_count(), 1);

    // And the last one.
    client.unstake(&s2, &xlm, &50);
    assert_eq!(client.staker_count(), 0);
}

#[test]
fn test_totals_update_on_emergency_unstake() {
    let lock_start = 0u64;
    let total_lock = 30u64 * 24 * 3600;
    let unlock = lock_start + total_lock;

    let (env, client, _admin, staker, _treasury) =
        setup_emergency(lock_start, unlock, 1_000_000);
    let asset = symbol_short!("XLM");

    // After setup_emergency: one staker, total_staked = 1_000_000.
    assert_eq!(client.total_staked(&asset), 1_000_000);
    assert_eq!(client.staker_count(), 1);

    // Partial emergency unstake keeps count above zero and reduces total by gross.
    env.ledger().set_timestamp(lock_start);
    client.emergency_unstake(&staker, &asset, &400_000);
    assert_eq!(client.total_staked(&asset), 600_000);
    assert_eq!(client.staker_count(), 1);

    // Full emergency exit returns total to zero AND drops staker count to zero.
    env.ledger().set_timestamp(lock_start + total_lock * 2 + 1);
    client.emergency_unstake(&staker, &asset, &600_000);
    assert_eq!(client.total_staked(&asset), 0);
    assert_eq!(client.staker_count(), 0);
}

#[test]
fn test_totals_handle_re_stake_after_full_exit() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.stake(&staker, &asset, &500);
    assert_eq!(client.staker_count(), 1);
    client.unstake(&staker, &asset, &500);
    assert_eq!(client.staker_count(), 0);
    assert_eq!(client.total_staked(&asset), 0);

    // Re-stake after full exit: counter and total come back.
    client.stake(&staker, &asset, &500);
    assert_eq!(client.staker_count(), 1);
    assert_eq!(client.total_staked(&asset), 500);
}

// ===========================================================================
// Stress tests: extreme rates, durations, and accuracy verification
// ===========================================================================

// -- helpers for stress tests -------------------------------------------------

/// Compute the reference daily-compounded yield using the exact formula:
///   yield = P * ((1 + APR/365)^days - 1)
/// where `days` is whole days only (matching the contract's whole-day exponent).
fn reference_daily_yield(principal: i128, apr_fp: i128, whole_days: u64) -> i128 {
    // (1 + apr/365)^days via repeated multiplication in fixed-point.
    let daily_rate = apr_fp / 365;
    let per_day = SCALE + daily_rate;
    let mut factor = SCALE; // 1.0
    let mut base = per_day;
    let mut exp = whole_days;
    while exp > 0 {
        if exp & 1 == 1 {
            factor = (factor as i128 * base as i128) / SCALE;
        }
        exp >>= 1;
        if exp > 0 {
            base = (base as i128 * base as i128) / SCALE;
        }
    }
    let growth = factor - SCALE;
    (principal as i128 * growth as i128) / SCALE
}

/// Compute the reference continuous-compounded yield:
///   yield = P * (e^(APR * t) - 1)
/// Uses the contract's `fp::exp` for the same precision.
fn reference_continuous_yield(principal: i128, apr_fp: i128, duration_secs: u64) -> i128 {
    use crate::fixed_point as fp;
    let t = fp::div(duration_secs as i128, SECONDS_PER_YEAR as i128).unwrap();
    let exponent = fp::mul(apr_fp, t).unwrap();
    let factor = fp::exp(exponent).unwrap();
    let growth = factor - SCALE;
    (principal as i128 * growth as i128) / SCALE
}

/// Allow ±0.01% tolerance on a reference value (industry standard).
fn assert_within_bps(actual: i128, reference: i128, bps: i128) {
    let tol = (reference.abs() * bps) / 10_000;
    let diff = (actual - reference).abs();
    assert!(
        diff <= tol.max(1),
        "assert_within_bps failed: actual={}, reference={}, bps tolerance={}, diff={}",
        actual, reference, bps, diff,
    );
}

// ---------------------------------------------------------------------------
// Extreme APR stress tests
// ---------------------------------------------------------------------------

#[test]
fn stress_daily_100pct_apr_one_year() {
    // 100% APR daily compounding for 1 year.
    // Expected: P * ((1 + 1/365)^365 - 1) ≈ P * 1.7141
    let principal = 1_000_000_000i128;
    let apr = SCALE; // 100% = 1.0 in fixed-point
    let calc = crate::compounding::YieldCalculator::new(crate::compounding::Compounding::Daily);
    let earned = calc.compute_yield(principal, apr, SECONDS_PER_YEAR).unwrap();
    let ref_earned = reference_daily_yield(principal, apr, 365);
    assert_within_bps(earned, ref_earned, 1); // within 0.01%
    assert!(earned > principal, "100% APR over 1 year should more than double: earned={}", earned);
}

#[test]
fn stress_continuous_100pct_apr_one_year() {
    // 100% APR continuous compounding for 1 year.
    // Expected: P * (e^1 - 1) ≈ P * 1.7183
    let principal = 1_000_000_000i128;
    let apr = SCALE; // 100%
    let calc = crate::compounding::YieldCalculator::new(crate::compounding::Compounding::Continuous);
    let earned = calc.compute_yield(principal, apr, SECONDS_PER_YEAR).unwrap();
    let ref_earned = reference_continuous_yield(principal, apr, SECONDS_PER_YEAR);
    assert_within_bps(earned, ref_earned, 1);
    // e^1 - 1 = 1.71828..., so yield should be ~1.718x principal.
    assert!(earned > principal, "continuous 100% APR over 1 year: earned={}", earned);
}

#[test]
fn stress_very_low_apr() {
    // 0.001% APR daily compounding for 1 year.
    let principal = 1_000_000_000i128;
    let apr = SCALE / 100_000; // 0.00001 = 0.001%
    let calc = crate::compounding::YieldCalculator::new(crate::compounding::Compounding::Daily);
    let earned = calc.compute_yield(principal, apr, SECONDS_PER_YEAR).unwrap();
    // Simple interest approximation: P * 0.001% = 1000
    // Compounding adds a tiny bit, so earned ≈ 1000.
    assert!(earned > 0, "should earn something at 0.001% APR");
    assert!(earned < 2000, "0.001% on 1B should be ~1000, got {}", earned);
}

#[test]
fn stress_very_low_apr_continuous() {
    // 0.001% APR continuous for 1 year.
    let principal = 1_000_000_000i128;
    let apr = SCALE / 100_000;
    let calc = crate::compounding::YieldCalculator::new(crate::compounding::Compounding::Continuous);
    let earned = calc.compute_yield(principal, apr, SECONDS_PER_YEAR).unwrap();
    assert!(earned > 0, "should earn something");
    assert!(earned < 2000, "too high for 0.001%: {}", earned);
}

// ---------------------------------------------------------------------------
// Extreme duration stress tests
// ---------------------------------------------------------------------------

#[test]
fn stress_one_second_duration() {
    // 1 second at 5% APR should produce a tiny but non-negative yield.
    let principal = 1_000_000_000_000i128; // 1 trillion
    let apr = SCALE / 20; // 5%
    let calc = crate::compounding::YieldCalculator::new(crate::compounding::Compounding::Daily);
    let earned = calc.compute_yield(principal, apr, 1).unwrap();
    // 1 second at 5% ≈ 1e12 * 0.05 / (365*86400) ≈ 158
    assert!(earned >= 0, "should not be negative: {}", earned);
    assert!(earned < 1000, "1 second at 5% should be tiny: {}", earned);
}

#[test]
fn stress_one_second_continuous() {
    let principal = 1_000_000_000_000i128;
    let apr = SCALE / 20; // 5%
    let calc = crate::compounding::YieldCalculator::new(crate::compounding::Compounding::Continuous);
    let earned = calc.compute_yield(principal, apr, 1).unwrap();
    assert!(earned >= 0);
    assert!(earned < 1000, "1 second continuous: {}", earned);
}

#[test]
fn stress_five_year_duration() {
    // 10% APR daily over 5 years.
    let principal = 100_000_000_000i128;
    let apr = SCALE / 10; // 10%
    let five_years = 5 * SECONDS_PER_YEAR;
    let calc = crate::compounding::YieldCalculator::new(crate::compounding::Compounding::Daily);
    let earned = calc.compute_yield(principal, apr, five_years).unwrap();
    // Simple interest would give P * 0.10 * 5 = P * 0.50 = 50B.
    // Compounding should exceed that.
    assert!(earned > 50_000_000_000, "5yr 10% should exceed simple: {}", earned);
    assert!(earned < 100_000_000_000, "5yr 10% should be < 100%: {}", earned);
}

#[test]
fn stress_ten_year_duration_continuous() {
    // 5% APR continuous over 10 years.
    let principal = 100_000_000_000i128;
    let apr = SCALE / 20; // 5%
    let ten_years = 10 * SECONDS_PER_YEAR;
    let calc = crate::compounding::YieldCalculator::new(crate::compounding::Compounding::Continuous);
    let earned = calc.compute_yield(principal, apr, ten_years).unwrap();
    // e^(0.05*10) - 1 = e^0.5 - 1 ≈ 0.6487
    // expected yield ≈ 64.87B
    let ref_earned = reference_continuous_yield(principal, apr, ten_years);
    assert_within_bps(earned, ref_earned, 1);
    assert!(earned > 60_000_000_000, "10yr 5% continuous: {}", earned);
    assert!(earned < 70_000_000_000, "10yr 5% continuous: {}", earned);
}

// ---------------------------------------------------------------------------
// Continuous >= Daily invariant
// ---------------------------------------------------------------------------

#[test]
fn stress_continuous_never_less_than_daily_various_rates() {
    let principal = 1_000_000_000i128;
    let rates = [
        SCALE / 100,     // 1%
        SCALE / 10,      // 10%
        SCALE / 4,       // 25%
        SCALE / 2,       // 50%
        SCALE,            // 100%
        SCALE * 2,       // 200%
        SCALE * 10,      // 1000%
    ];
    for &apr in &rates {
        for &dur in &[SECONDS_PER_DAY, 30 * SECONDS_PER_DAY, SECONDS_PER_YEAR] {
            let daily = crate::compounding::YieldCalculator::new(crate::compounding::Compounding::Daily)
                .compute_yield(principal, apr, dur)
                .unwrap();
            let cont = crate::compounding::YieldCalculator::new(crate::compounding::Compounding::Continuous)
                .compute_yield(principal, apr, dur)
                .unwrap();
            assert!(
                cont >= daily,
                "continuous ({}) should be >= daily ({}) for apr={}, dur={}",
                cont, daily, apr, dur,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Accuracy vs reference formulas: ±0.01% (1 bps)
// ---------------------------------------------------------------------------

#[test]
fn accuracy_daily_matches_reference_various_aprs() {
    let principal = 1_000_000_000_000i128; // 1T
    let rates = [
        (SCALE / 100, "1%"),
        (SCALE / 20, "5%"),
        (SCALE / 10, "10%"),
        (SCALE / 4, "25%"),
        (SCALE / 2, "50%"),
    ];
    let durations = [
        (SECONDS_PER_DAY, "1 day"),
        (7 * SECONDS_PER_DAY, "7 days"),
        (30 * SECONDS_PER_DAY, "30 days"),
        (SECONDS_PER_YEAR, "1 year"),
    ];
    let calc = crate::compounding::YieldCalculator::new(crate::compounding::Compounding::Daily);
    for &(apr, apr_label) in &rates {
        for &(dur, dur_label) in &durations {
            let earned = calc.compute_yield(principal, apr, dur).unwrap();
            let whole_days = dur / SECONDS_PER_DAY;
            let ref_earned = reference_daily_yield(principal, apr, whole_days);
            assert_within_bps(earned, ref_earned, 1);
        }
    }
}

#[test]
fn accuracy_continuous_matches_reference_various_aprs() {
    let principal = 1_000_000_000_000i128;
    let rates = [
        (SCALE / 100, "1%"),
        (SCALE / 20, "5%"),
        (SCALE / 10, "10%"),
        (SCALE / 4, "25%"),
        (SCALE / 2, "50%"),
    ];
    let durations = [
        SECONDS_PER_DAY,
        7 * SECONDS_PER_DAY,
        30 * SECONDS_PER_DAY,
        SECONDS_PER_YEAR,
    ];
    let calc = crate::compounding::YieldCalculator::new(crate::compounding::Compounding::Continuous);
    for &(apr, _) in &rates {
        for &dur in &durations {
            let earned = calc.compute_yield(principal, apr, dur).unwrap();
            let ref_earned = reference_continuous_yield(principal, apr, dur);
            assert_within_bps(earned, ref_earned, 1);
        }
    }
}

// ---------------------------------------------------------------------------
// APR/APY roundtrip accuracy at extreme values
// ---------------------------------------------------------------------------

#[test]
fn apy_roundtrip_extreme_low_apr() {
    let apr = SCALE / 10_000; // 0.01%
    for mode in &[CompoundingMode::Daily, CompoundingMode::Continuous] {
        let apy = client_for_roundtrip_apr_to_apy(apr, *mode);
        let back = client_for_roundtrip_apy_to_apr(apy, *mode);
        // 0.01% target: tolerance ~ SCALE / 10_000_000
        approx(back, apr, SCALE / 10_000_000);
    }
}

#[test]
fn apy_roundtrip_high_apr() {
    let apr = SCALE * 2; // 200%
    for mode in &[CompoundingMode::Daily, CompoundingMode::Continuous] {
        let apy = client_for_roundtrip_apr_to_apy(apr, *mode);
        let back = client_for_roundtrip_apy_to_apr(apy, *mode);
        approx(back, apr, SCALE / 1000); // 0.1% tolerance at high rates
    }
}

#[test]
fn apy_roundtrip_extreme_high_apr() {
    let apr = SCALE * 10; // 1000%
    for mode in &[CompoundingMode::Daily, CompoundingMode::Continuous] {
        let apy = client_for_roundtrip_apr_to_apy(apr, *mode);
        let back = client_for_roundtrip_apy_to_apr(apy, *mode);
        approx(back, apr, SCALE / 100); // 1% tolerance at extreme rates
    }
}

// ---------------------------------------------------------------------------
// Time-weighted accrual: rate changes preserve correctness
// ---------------------------------------------------------------------------

#[test]
fn time_weighted_rate_change_daily() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    let principal = 1_000_000_000i128;

    // Phase 1: 10% APR for 180 days.
    let apr1 = SCALE / 10;
    client.open_yield_position(&staker, &asset, &principal, &apr1, &CompoundingMode::Daily);
    env.ledger().set_timestamp(180 * SECONDS_PER_DAY);
    let mid = client.accrue_yield(&staker, &asset);
    let yield_phase1 = mid.accrued_yield;
    assert!(yield_phase1 > 0, "phase 1 should accrue yield");

    // Phase 2: change to 5% APR for another 180 days.
    let apr2 = SCALE / 20;
    client.set_yield_rate(&staker, &asset, &apr2);
    env.ledger().set_timestamp(365 * SECONDS_PER_DAY);
    let full = client.accrue_yield(&staker, &asset);
    let yield_phase2 = full.accrued_yield - yield_phase1;
    assert!(yield_phase2 > 0, "phase 2 should accrue yield");

    // Verify: phase 1 yield should match a 10%-APR 180-day calc.
    let ref_phase1 = reference_daily_yield(principal, apr1, 180);
    assert_within_bps(yield_phase1, ref_phase1, 1);

    // Verify: phase 2 yield should match a 5%-APR 185-day calc
    // (185 days from day 180 to day 365).
    let ref_phase2 = reference_daily_yield(principal, apr2, 185);
    assert_within_bps(yield_phase2, ref_phase2, 1);
}

#[test]
fn time_weighted_multiple_rate_changes() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    let principal = 10_000_000i128;

    client.open_yield_position(&staker, &asset, &principal, &SCALE, &CompoundingMode::Continuous);
    // 20% for 100 days
    env.ledger().set_timestamp(100 * SECONDS_PER_DAY);
    client.set_yield_rate(&staker, &asset, &(SCALE / 5));
    // 20% for another 100 days
    env.ledger().set_timestamp(200 * SECONDS_PER_DAY);
    client.set_yield_rate(&staker, &asset, &(SCALE / 10));
    // 10% for 165 more days
    env.ledger().set_timestamp(365 * SECONDS_PER_DAY);
    let record = client.accrue_yield(&staker, &asset);
    assert!(record.accrued_yield > 0, "must have yield");
}

// ---------------------------------------------------------------------------
// current_yield is read-only (no state mutation)
// ---------------------------------------------------------------------------

#[test]
fn current_yield_does_not_mutate_state() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    client.open_yield_position(&staker, &asset, &1_000_000, &(SCALE / 10), &CompoundingMode::Daily);

    env.ledger().set_timestamp(SECONDS_PER_DAY * 30);
    let y1 = client.current_yield(&staker, &asset);
    let y2 = client.current_yield(&staker, &asset);
    assert_eq!(y1, y2, "repeated current_yield calls should return the same value");
    assert!(y1 > 0, "should have some yield after 30 days");
}

// ---------------------------------------------------------------------------
// History is maintained and queryable across accruals
// ---------------------------------------------------------------------------

#[test]
fn history_grows_with_each_accrual() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    client.open_yield_position(&staker, &asset, &1_000_000, &(SCALE / 10), &CompoundingMode::Daily);

    // First accrual at day 30.
    env.ledger().set_timestamp(30 * SECONDS_PER_DAY);
    client.accrue_yield(&staker, &asset);
    let h1 = client.yield_history(&staker, &asset);
    assert_eq!(h1.len(), 1, "should have 1 history entry");

    // Second accrual at day 60.
    env.ledger().set_timestamp(60 * SECONDS_PER_DAY);
    client.accrue_yield(&staker, &asset);
    let h2 = client.yield_history(&staker, &asset);
    assert_eq!(h2.len(), 2, "should have 2 history entries");

    // Each entry should have yield_earned > 0 and period_seconds > 0.
    for i in 0..h2.len() {
        let entry = h2.get(i).unwrap();
        assert!(entry.yield_earned > 0, "entry {} should have earned yield", i);
        assert!(entry.period_seconds > 0, "entry {} should have positive period", i);
    }
}

#[test]
fn history_records_rate_change_periods() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    client.open_yield_position(&staker, &asset, &1_000_000, &(SCALE / 10), &CompoundingMode::Daily);

    // Accrue at 10%, then change rate.
    env.ledger().set_timestamp(100 * SECONDS_PER_DAY);
    client.accrue_yield(&staker, &asset);
    client.set_yield_rate(&staker, &asset, &(SCALE / 20)); // 5%
    env.ledger().set_timestamp(200 * SECONDS_PER_DAY);
    client.accrue_yield(&staker, &asset);

    let history = client.yield_history(&staker, &asset);
    // First entry: 10% APR over ~100 days.
    assert_eq!(history.get(0).unwrap().apr, SCALE / 10);
    // Second entry: 5% APR over ~100 days (the set_yield_rate call logs a history entry too).
    // set_yield_rate triggers accrue_to which appends the first segment, then
    // the explicit accrue appends the second segment.
    assert!(history.len() >= 2);
    // Check the last entry has the new rate.
    let last = history.get(history.len() - 1).unwrap();
    assert_eq!(last.apr, SCALE / 20);
}

// ---------------------------------------------------------------------------
// Projection accuracy at various horizons
// ---------------------------------------------------------------------------

#[test]
fn projection_accuracy_30_days() {
    let principal = SCALE; // 1 unit
    let apr = SCALE / 10; // 10%
    let horizon = 30 * SECONDS_PER_DAY;
    let proj = client_project(principal, apr, CompoundingMode::Continuous, horizon);
    let ref_yield = reference_continuous_yield(principal, apr, horizon);
    assert_within_bps(proj.projected_yield, ref_yield, 1);
}

#[test]
fn projection_accuracy_1_year() {
    let principal = SCALE * 100;
    let apr = SCALE / 20; // 5%
    let proj = client_project(principal, apr, CompoundingMode::Daily, SECONDS_PER_YEAR);
    let ref_yield = reference_daily_yield(principal, apr, 365);
    assert_within_bps(proj.projected_yield, ref_yield, 1);}

#[test]
fn projection_balance_equals_principal_plus_yield() {
    let principal = 500_000_000i128;
    let apr = SCALE / 5; // 20%
    let proj = client_project(principal, apr, CompoundingMode::Continuous, SECONDS_PER_YEAR);
    assert_eq!(proj.projected_balance, principal + proj.projected_yield);}

// ---------------------------------------------------------------------------
// Zero principal edge cases
// ---------------------------------------------------------------------------

#[test]
fn zero_principal_yields_zero_all_modes() {
    for mode in &[CompoundingMode::Daily, CompoundingMode::Continuous] {
        let calc = crate::compounding::YieldCalculator::new(mode.to_strategy());
        assert_eq!(calc.compute_yield(0, SCALE / 10, SECONDS_PER_YEAR).unwrap(), 0);
        assert_eq!(calc.compute_balance(0, SCALE / 10, SECONDS_PER_YEAR).unwrap(), 0);
    }
}

#[test]
fn zero_apr_yields_zero_all_modes() {
    let principal = 1_000_000i128;
    for mode in &[CompoundingMode::Daily, CompoundingMode::Continuous] {
        let calc = crate::compounding::YieldCalculator::new(mode.to_strategy());
        assert_eq!(calc.compute_yield(principal, 0, SECONDS_PER_YEAR).unwrap(), 0);
    }
}

#[test]
fn zero_duration_yields_zero_all_modes() {
    let principal = 1_000_000i128;
    for mode in &[CompoundingMode::Daily, CompoundingMode::Continuous] {
        let calc = crate::compounding::YieldCalculator::new(mode.to_strategy());
        assert_eq!(calc.compute_yield(principal, SCALE / 10, 0).unwrap(), 0);
    }
}

// ---------------------------------------------------------------------------
// Segment yield computation (variable rates)
// ---------------------------------------------------------------------------

#[test]
fn yield_segments_time_weighted_accuracy() {
    let principal = 1_000_000_000i128;
    let calc = crate::compounding::YieldCalculator::new(crate::compounding::Compounding::Daily);
    // 10% for 180 days, then 5% for 185 days.
    let segments = [
        (SCALE / 10, 180 * SECONDS_PER_DAY),
        (SCALE / 20, 185 * SECONDS_PER_DAY),
    ];
    let total_yield = calc.compute_yield_segments(principal, &segments).unwrap();
    assert!(total_yield > 0, "segments should produce yield");
    // Verify by computing manually: compound through each segment.
    let mut balance = principal;
    for &(apr, dur) in &segments {
        let earned = calc.compute_yield(balance, apr, dur).unwrap();
        balance += earned;
    }
    assert_eq!(total_yield, balance - principal);
}

// ---------------------------------------------------------------------------n// Yield claim resets accrued_yield
// ---------------------------------------------------------------------------

#[test]
fn claim_full_cycle_accrue_claim_reaccrue() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    client.open_yield_position(&staker, &asset, &1_000_000, &(SCALE / 10), &CompoundingMode::Daily);

    // Accrue 30 days, claim, then accrue another 30 days.
    env.ledger().set_timestamp(30 * SECONDS_PER_DAY);
    let first_claim = client.claim_yield(&staker, &asset);
    assert!(first_claim > 0);
    assert_eq!(client.current_yield(&staker, &asset), 0);

    env.ledger().set_timestamp(60 * SECONDS_PER_DAY);
    let second_yield = client.current_yield(&staker, &asset);
    assert!(second_yield > 0, "should accrue again after claim");

    // Second yield should be approximately the same as the first (same 30-day window).
    assert_within_bps(second_yield, first_claim, 100); // 1% tolerance for compounding effect
}

// ---------------------------------------------------------------------------
// Precision: 18-digit fixed-point doesn't lose precision on small yields
// ---------------------------------------------------------------------------

#[test]
fn precision_small_yield_on_large_principal() {
    // Very low rate, short time — tests that fixed-point doesn't round to zero.
    let principal = 1_000_000_000_000_000_000i128; // 1e18 (= SCALE)
    let apr = SCALE / 1_000_000; // 0.0001% = 1e-6
    let calc = crate::compounding::YieldCalculator::new(crate::compounding::Compounding::Continuous);
    let earned = calc.compute_yield(principal, apr, SECONDS_PER_DAY).unwrap();
    // Expected: ~1e18 * 1e-6 / 365 ≈ 2.74e9
    assert!(earned > 0, "small yield should be non-zero: {}", earned);
}

// ===========================================================================
// Helper wrappers (use contract client for roundtrip tests)
// ===========================================================================

fn client_for_roundtrip_apr_to_apy(apr: i128, mode: CompoundingMode) -> i128 {
    let (_env, client) = setup();
    client.apr_to_apy(&apr, &mode)
}

fn client_for_roundtrip_apy_to_apr(apy: i128, mode: CompoundingMode) -> i128 {
    let (_env, client) = setup();
    client.apy_to_apr(&apy, &mode)
}

fn client_project(
    principal: i128,
    apr: i128,
    mode: CompoundingMode,
    horizon: u64,
) -> crate::records::YieldProjection {
    let (_env, client) = setup();
    client.project_yield(&principal, &apr, &mode, &horizon)
}
