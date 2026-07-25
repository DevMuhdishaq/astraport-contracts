//! Unit and contract-level tests for the multi-asset staking contract.
//!
//! Tests cover:
//! - Contract lifecycle (initialize, admin checks)
//! - Multi-asset stake / unstake / balance
//! - Unlock schedule enforcement (Immediate, Cliff, Graduated)
//! - Yield accrual, rate changes, history, claims
//! - Portfolio aggregation (snapshot, weighted APR, total yield)
//! - APR ↔ APY conversions and projections
//! - Distribution scheduling

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{symbol_short, Address, Env};

use crate::fixed_point::{SCALE, SECONDS_PER_DAY, SECONDS_PER_YEAR};
use crate::records::{
    CompoundingMode, GraduatedUnlock, StakeDataKey, UnlockSchedule, YieldDataKey,
};

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
// Lifecycle
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
    let (env, client, admin) = setup_with_admin();
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
fn test_stake_and_unstake_single_asset() {
    let (env, client) = setup();
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    assert_eq!(client.stake(&staker, &asset, &1_000), 1_000);
    assert_eq!(client.get_balance(&staker, &asset), 1_000);

    assert_eq!(client.stake(&staker, &asset, &500), 1_500);
    assert_eq!(client.get_balance(&staker, &asset), 1_500);

    assert_eq!(client.unstake(&staker, &asset, &600), 900);
    assert_eq!(client.get_balance(&staker, &asset), 900);

    assert_eq!(client.unstake(&staker, &asset, &900), 0);
    assert_eq!(client.get_balance(&staker, &asset), 0);
}

#[test]
fn test_stake_multiple_assets_independently() {
    let (env, client) = setup();
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
#[should_panic(expected = "InvalidStakeAmount")]
fn test_stake_zero_panics() {
    let (env, client) = setup();
    client.stake(&Address::generate(&env), &symbol_short!("XLM"), &0);
}

#[test]
#[should_panic(expected = "InvalidStakeAmount")]
fn test_stake_negative_panics() {
    let (env, client) = setup();
    client.stake(&Address::generate(&env), &symbol_short!("XLM"), &-1);
}

#[test]
#[should_panic(expected = "InsufficientBalance")]
fn test_unstake_more_than_balance_panics() {
    let (env, client) = setup();
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    client.stake(&staker, &asset, &100);
    client.unstake(&staker, &asset, &200);
}

// ---------------------------------------------------------------------------
// Staker asset list / portfolio
// ---------------------------------------------------------------------------

#[test]
fn test_staker_assets_tracked() {
    let (env, client) = setup();
    let staker = Address::generate(&env);

    client.stake(&staker, &symbol_short!("XLM"), &100);
    client.stake(&staker, &symbol_short!("USDC"), &200);
    client.stake(&staker, &symbol_short!("ETH"), &300);

    let assets = client.staker_assets(&staker);
    assert_eq!(assets.len(), 3);
}

#[test]
fn test_full_unstake_removes_asset_from_list() {
    let (env, client) = setup();
    let staker = Address::generate(&env);
    let xlm = symbol_short!("XLM");
    let usdc = symbol_short!("USDC");

    client.stake(&staker, &xlm, &100);
    client.stake(&staker, &usdc, &200);

    assert_eq!(client.staker_assets(&staker).len(), 2);

    client.unstake(&staker, &xlm, &100); // full exit
    assert_eq!(client.staker_assets(&staker).len(), 1);
}

#[test]
fn test_portfolio_snapshot_aggregates_positions() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);

    client.stake(&staker, &symbol_short!("XLM"), &1_000);
    client.stake(&staker, &symbol_short!("USDC"), &2_000);

    let snap = client.get_portfolio(&staker);
    assert_eq!(snap.total_principal, 3_000);
    assert_eq!(snap.asset_count, 2);
}

#[test]
fn test_portfolio_yield_sums_all_assets() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let xlm = symbol_short!("XLM");
    let usdc = symbol_short!("USDC");

    client.stake(&staker, &xlm, &SCALE);
    client.stake(&staker, &usdc, &SCALE);

    env.ledger().set_timestamp(SECONDS_PER_YEAR);

    // Each position earns ~5% daily on 1e18 principal: ~5.1267e16 each.
    let total = client.portfolio_yield(&staker);
    approx(total, 2 * 51_267_496_505_408_400i128, 1_000_000_000_000);
}

// ---------------------------------------------------------------------------
// Asset configuration / heterogeneous yield rates
// ---------------------------------------------------------------------------

#[test]
fn test_configure_asset_sets_custom_apr() {
    let (env, client, admin) = setup_with_admin();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("ETH");

    // Configure ETH at 20% APR continuous.
    client.configure_asset(
        &admin,
        &asset,
        &(SCALE / 5),
        &CompoundingMode::Continuous,
        &0,
        &0,
        &UnlockSchedule::Immediate,
    );
    client.stake(&staker, &asset, &SCALE);

    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    let rec = client.accrue_yield(&staker, &asset);
    // e^0.2 - 1 = 0.22140275816...
    approx(rec.accrued_yield, 221_402_758_160_169_833, 1_000_000_000_000);
}

#[test]
fn test_different_assets_earn_different_rates() {
    let (env, client, admin) = setup_with_admin();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);

    let low = symbol_short!("LOWRATE");
    let high = symbol_short!("HRATE");

    client.configure_asset(
        &admin,
        &low,
        &(SCALE / 100),          // 1% APR
        &CompoundingMode::Continuous,
        &0, &0, &UnlockSchedule::Immediate,
    );
    client.configure_asset(
        &admin,
        &high,
        &(SCALE / 5),            // 20% APR
        &CompoundingMode::Continuous,
        &0, &0, &UnlockSchedule::Immediate,
    );

    client.stake(&staker, &low, &SCALE);
    client.stake(&staker, &high, &SCALE);

    env.ledger().set_timestamp(SECONDS_PER_YEAR);

    let low_yield = client.current_yield(&staker, &low);
    let high_yield = client.current_yield(&staker, &high);

    assert!(
        high_yield > low_yield,
        "high-rate asset should earn more: {} vs {}",
        high_yield,
        low_yield
    );
    // e^0.01 - 1 ≈ 0.01005017
    approx(low_yield, 10_050_167_084_168_058, 1_000_000_000_000);
    // e^0.2 - 1 ≈ 0.22140275
    approx(high_yield, 221_402_758_160_169_833, 1_000_000_000_000);
}

#[test]
fn test_set_yield_defaults_applies_to_new_positions() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    // Override default to 10% continuous before first stake.
    client.set_yield_defaults(&(SCALE / 10), &CompoundingMode::Continuous);
    client.stake(&staker, &asset, &SCALE);

    env.ledger().set_timestamp(SECONDS_PER_YEAR / 2);
    // e^(0.1 * 0.5) - 1 = e^0.05 - 1 ≈ 0.05127
    approx(
        client.current_yield(&staker, &asset),
        51_271_096_376_024_040,
        100_000_000_000,
    );
}

#[test]
fn test_min_stake_enforcement() {
    let (env, client, admin) = setup_with_admin();
    let staker = Address::generate(&env);
    let asset = symbol_short!("ELITE");

    client.configure_asset(
        &admin,
        &asset,
        &DEFAULT_APR,
        &CompoundingMode::Daily,
        &500,   // min_stake = 500
        &0,
        &UnlockSchedule::Immediate,
    );

    // Below minimum — should panic with BelowMinimumStake.
    let result = client.try_stake(&staker, &asset, &100);
    assert!(result.is_err());
}

#[test]
fn test_max_stake_enforcement() {
    let (env, client, admin) = setup_with_admin();
    let staker = Address::generate(&env);
    let asset = symbol_short!("CAPPED");

    client.configure_asset(
        &admin,
        &asset,
        &DEFAULT_APR,
        &CompoundingMode::Daily,
        &0,
        &1_000,  // max_stake = 1_000
        &UnlockSchedule::Immediate,
    );

    client.stake(&staker, &asset, &1_000);
    // One more unit would exceed the cap.
    let result = client.try_stake(&staker, &asset, &1);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Unlock schedules
// ---------------------------------------------------------------------------

#[test]
fn test_cliff_unlock_blocks_early_withdrawal() {
    let (env, client, admin) = setup_with_admin();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("VESTED");

    // Cliff unlocks at t = 1 year.
    client.configure_asset(
        &admin,
        &asset,
        &DEFAULT_APR,
        &CompoundingMode::Daily,
        &0, &0,
        &UnlockSchedule::Cliff(SECONDS_PER_YEAR),
    );
    client.stake(&staker, &asset, &1_000);

    // Before the cliff — should fail.
    env.ledger().set_timestamp(SECONDS_PER_YEAR - 1);
    let result = client.try_unstake(&staker, &asset, &1);
    assert!(result.is_err());

    // At the cliff — should succeed.
    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    assert_eq!(client.unstake(&staker, &asset, &1_000), 0);
}

#[test]
fn test_graduated_unlock_partial_withdrawal() {
    let (env, client, admin) = setup_with_admin();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("GRAD");

    // 25% (2500 bps) per 90-day tranche: full unlock after 4 tranches.
    let grad = GraduatedUnlock {
        start_ts: 0,
        interval_seconds: 90 * SECONDS_PER_DAY,
        tranche_pct_bps: 2500,
    };
    client.configure_asset(
        &admin,
        &asset,
        &DEFAULT_APR,
        &CompoundingMode::Daily,
        &0, &0,
        &UnlockSchedule::Graduated(grad),
    );
    client.stake(&staker, &asset, &1_000);

    // At t = 0 the first tranche (25%) has just unlocked.
    assert_eq!(client.unstake(&staker, &asset, &250), 750);

    // After tranche 2 (t = 90 days), another 25% of original is unlocked.
    // Remaining principal = 750; unlocked from original = 50% of 1000 = 500.
    // Available to withdraw = 500 - already withdrawn 250 = … but the engine
    // tracks current principal (750), and 50% of 1000 = 500, which is less
    // than 750, so the unlocked cap is 500.
    env.ledger().set_timestamp(90 * SECONDS_PER_DAY);
    // 500 unlocked of original, 250 already withdrawn → 250 more available.
    assert_eq!(client.unstake(&staker, &asset, &250), 500);
}

#[test]
fn test_graduated_unlock_full_after_all_tranches() {
    let (env, client, admin) = setup_with_admin();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("GFULL");

    let grad = GraduatedUnlock {
        start_ts: 0,
        interval_seconds: SECONDS_PER_DAY,
        tranche_pct_bps: 2500, // 4 tranches
    };
    client.configure_asset(
        &admin,
        &asset,
        &DEFAULT_APR,
        &CompoundingMode::Daily,
        &0, &0,
        &UnlockSchedule::Graduated(grad),
    );
    client.stake(&staker, &asset, &1_000);

    // After 4 days all tranches are done → full withdrawal allowed.
    env.ledger().set_timestamp(4 * SECONDS_PER_DAY);
    assert_eq!(client.unstake(&staker, &asset, &1_000), 0);
}

#[test]
fn test_immediate_unlock_allows_instant_withdrawal() {
    let (env, client) = setup();
    let staker = Address::generate(&env);
    let asset = symbol_short!("FREE");
    client.stake(&staker, &asset, &1_000);
    assert_eq!(client.unstake(&staker, &asset, &1_000), 0);
}

// ---------------------------------------------------------------------------
// Yield accrual across multiple assets
// ---------------------------------------------------------------------------

#[test]
fn test_stake_opens_position_and_accrues_on_staked_principal() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    assert_eq!(client.stake(&staker, &asset, &SCALE), SCALE);
    assert_eq!(client.get_balance(&staker, &asset), SCALE);

    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    // 5% APR daily on 1e18: (1 + 0.05/365)^365 - 1 ≈ 5.1267e16
    approx(
        client.current_yield(&staker, &asset),
        51_267_496_505_408_400,
        100_000_000_000,
    );
}

#[test]
fn test_additional_stake_raises_principal_and_keeps_yield() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.stake(&staker, &asset, &SCALE);
    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    let new_bal = client.stake(&staker, &asset, &SCALE);
    assert_eq!(new_bal, 2 * SCALE);

    let rec = client.accrue_yield(&staker, &asset);
    assert_eq!(rec.principal, 2 * SCALE);
    approx(rec.accrued_yield, 51_267_496_505_408_400, 100_000_000_000);
}

#[test]
fn test_unstake_preserves_accrued_yield_and_reduces_principal() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.stake(&staker, &asset, &SCALE);
    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    let expected_yield = 51_267_496_505_408_400i128;

    assert_eq!(client.unstake(&staker, &asset, &(SCALE / 2)), SCALE / 2);

    let rec = client.accrue_yield(&staker, &asset);
    assert_eq!(rec.principal, SCALE / 2);
    approx(rec.accrued_yield, expected_yield, 100_000_000_000);
}

#[test]
fn test_rate_change_is_time_weighted() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    // Open at 4%, then switch to 8% at the halfway mark.
    client.open_yield_position(
        &staker, &asset, &SCALE, &(4 * SCALE / 100), &CompoundingMode::Continuous,
    );
    env.ledger().set_timestamp(SECONDS_PER_YEAR / 2);
    client.set_yield_rate(&staker, &asset, &(8 * SCALE / 100));

    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    let rec = client.accrue_yield(&staker, &asset);

    let seg1 = 20_201_340_026_755_810i128; // e^0.02 - 1
    let seg2 = 40_810_774_192_388_960i128; // e^0.04 - 1
    approx(rec.accrued_yield, seg1 + seg2, 10_000_000_000);
}

#[test]
fn test_accrue_claim_resets_unclaimed_yield() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.open_yield_position(
        &staker, &asset, &SCALE, &(SCALE / 10), &CompoundingMode::Daily,
    );
    env.ledger().set_timestamp(30 * SECONDS_PER_DAY);

    let accrued = client.accrue_yield(&staker, &asset).accrued_yield;
    assert!(accrued > 0);
    assert_eq!(client.claim_yield(&staker, &asset), accrued);
    assert_eq!(client.current_yield(&staker, &asset), 0);
    // Double claim returns zero.
    assert_eq!(client.claim_yield(&staker, &asset), 0);
}

#[test]
fn test_history_is_complete_and_queryable() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.open_yield_position(
        &staker, &asset, &SCALE, &(SCALE / 20), &CompoundingMode::Daily,
    );
    env.ledger().set_timestamp(30 * SECONDS_PER_DAY);
    client.accrue_yield(&staker, &asset);
    env.ledger().set_timestamp(60 * SECONDS_PER_DAY);
    client.accrue_yield(&staker, &asset);
    env.ledger().set_timestamp(90 * SECONDS_PER_DAY);
    let rec = client.accrue_yield(&staker, &asset);

    let history = client.yield_history(&staker, &asset);
    assert_eq!(history.len(), 3);
    assert_eq!(history.get(2).unwrap().cumulative_yield, rec.accrued_yield);
    assert_eq!(history.get(0).unwrap().period_seconds, 30 * SECONDS_PER_DAY);
}

// ---------------------------------------------------------------------------
// APR ↔ APY conversions and projections
// ---------------------------------------------------------------------------

#[test]
fn test_apr_to_apy_known_values() {
    let (_env, client) = setup();
    approx(
        client.apr_to_apy(&(SCALE / 20), &CompoundingMode::Daily),
        51_267_496_505_408_400,
        100_000_000_000,
    );
    approx(
        client.apr_to_apy(&(SCALE / 20), &CompoundingMode::Continuous),
        51_271_096_376_024_040,
        100_000_000_000,
    );
}

#[test]
fn test_apy_apr_roundtrip() {
    let (_env, client) = setup();
    let apr = SCALE / 10;
    let apy = client.apr_to_apy(&apr, &CompoundingMode::Daily);
    approx(client.apy_to_apr(&apy, &CompoundingMode::Daily), apr, 100_000_000_000_000);
}

#[test]
fn test_thirty_day_projection_within_one_percent() {
    let (_env, client) = setup();
    let horizon = 30 * SECONDS_PER_DAY;
    let proj = client.project_yield(&SCALE, &(SCALE / 10), &CompoundingMode::Continuous, &horizon);
    let expected = 8_253_048_640_000_000i128;
    let diff = (proj.projected_yield - expected).abs();
    assert!(
        diff <= expected / 100,
        "projection off by >1%: {} vs {}",
        proj.projected_yield,
        expected
    );
}

// ---------------------------------------------------------------------------
// Distribution scheduling
// ---------------------------------------------------------------------------

#[test]
fn test_one_off_distribution() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.schedule_distribution(&staker, &asset, &500, &2_000, &0);
    assert_eq!(client.process_distribution(&staker, &asset), 0);

    env.ledger().set_timestamp(2_000);
    assert_eq!(client.process_distribution(&staker, &asset), 500);

    env.ledger().set_timestamp(3_000);
    assert_eq!(client.process_distribution(&staker, &asset), 0);
}

#[test]
fn test_recurring_distribution_rolls_forward() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.schedule_distribution(&staker, &asset, &100, &1_000, &1_000);

    env.ledger().set_timestamp(1_000);
    assert_eq!(client.process_distribution(&staker, &asset), 100);

    env.ledger().set_timestamp(2_000);
    assert_eq!(client.process_distribution(&staker, &asset), 100);

    env.ledger().set_timestamp(2_500);
    assert_eq!(client.process_distribution(&staker, &asset), 0);
}

// ---------------------------------------------------------------------------
// Admin controls
// ---------------------------------------------------------------------------

#[test]
fn test_set_alert_threshold_requires_admin() {
    let (env, client, admin) = setup_with_admin();
    assert_eq!(client.set_alert_threshold(&admin, &10_000), symbol_short!("ok"));
}

#[test]
#[should_panic(expected = "caller is not admin")]
fn test_set_alert_threshold_non_admin_fails() {
    let (env, client, _admin) = setup_with_admin();
    client.set_alert_threshold(&Address::generate(&env), &10_000);
}

#[test]
#[should_panic]
fn test_claim_yield_requires_staker_auth() {
    let env = Env::default(); // no mock_all_auths
    let contract_id = env.register_contract(None, StakingContract);
    let client = StakingContractClient::new(&env, &contract_id);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    {
        // Open position with mocked auth.
        let env2 = Env::default();
        env2.mock_all_auths();
        let cid2 = env2.register_contract(None, StakingContract);
        let c2 = StakingContractClient::new(&env2, &cid2);
        c2.open_yield_position(&staker, &asset, &SCALE, &(SCALE / 10), &CompoundingMode::Daily);
    }
    // Claim without auth — should panic.
    client.claim_yield(&staker, &asset);
}

#[test]
fn test_zero_principal_accrues_nothing() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    client.open_yield_position(&staker, &asset, &0, &(SCALE / 10), &CompoundingMode::Daily);
    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    assert_eq!(client.accrue_yield(&staker, &asset).accrued_yield, 0);
}
