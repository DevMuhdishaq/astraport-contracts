//! Unit and contract-level tests for the staking contract, yield engine, and
//! alert monitoring system.
//!
//! Tests are grouped into:
//! - Staking contract smoke tests (initialize, stake, unstake, balance).
//! - Alert system tests: config management, threshold evaluation, history,
//!   acknowledgment, severity levels, and event delivery.
//! - Yield engine tests: accrual, rate changes, history, projections.
//!
//! Pure-math correctness tests live alongside their implementations in
//! `fixed_point`, `compounding`, `apy`, and `projection`.

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{symbol_short, vec, Address, Env, String, Vec};

use crate::alerts::{AlertKind, AlertSeverity, AlertThreshold};
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

fn make_threshold(
    env: &Env,
    kind: AlertKind,
    trigger_value: i128,
    severity: AlertSeverity,
    label: &str,
) -> AlertThreshold {
    AlertThreshold {
        kind,
        trigger_value,
        severity,
        label: String::from_str(env, label),
        enabled: true,
    }
}

// ---------------------------------------------------------------------------
// Staking smoke tests
// ---------------------------------------------------------------------------

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();
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
    let (env, client) = setup();
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
    let staker = Address::generate(&env);
    client.stake(&staker, &0);
}

#[test]
#[should_panic(expected = "InvalidStakeAmount")]
fn test_stake_negative() {
    let (env, client) = setup();
    let staker = Address::generate(&env);
    client.stake(&staker, &-50);
}

#[test]
#[should_panic(expected = "InsufficientBalance")]
fn test_unstake_more_than_balance() {
    let (env, client) = setup();
    let staker = Address::generate(&env);
    client.stake(&staker, &100);
    client.unstake(&staker, &150);
}

#[test]
fn test_set_alert_threshold_requires_admin_auth() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_alert_threshold(&admin, &10_000);
}

#[test]
#[should_panic(expected = "caller is not admin")]
fn test_set_alert_threshold_non_admin_fails() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let non_admin = Address::generate(&env);
    client.set_alert_threshold(&non_admin, &10_000);
}

// ---------------------------------------------------------------------------
// Alert config management tests
// ---------------------------------------------------------------------------

#[test]
fn alert_config_create_and_retrieve() {
    let (env, client) = setup();
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    let t = make_threshold(&env, AlertKind::BalanceDrop, 500, AlertSeverity::Warning, "low balance");
    let thresholds: Vec<AlertThreshold> = vec![&env, t];

    let cfg = client.set_alert_config(&staker, &asset, &thresholds, &true);
    assert_eq!(cfg.thresholds.len(), 1);
    assert!(cfg.alerts_enabled);

    let retrieved = client.get_alert_config(&staker, &asset).unwrap();
    assert_eq!(retrieved.thresholds.len(), 1);
    assert_eq!(retrieved.thresholds.get(0).unwrap().trigger_value, 500);
}

#[test]
fn add_and_remove_threshold() {
    let (env, client) = setup();
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    // Start with one threshold.
    let t1 = make_threshold(&env, AlertKind::BalanceDrop, 1000, AlertSeverity::Critical, "balance floor");
    client.set_alert_config(&staker, &asset, &vec![&env, t1], &true);

    // Add a second.
    let t2 = make_threshold(&env, AlertKind::YieldUnderperformance, SCALE / 100, AlertSeverity::Info, "low apr");
    let cfg = client.add_alert_threshold(&staker, &asset, &t2);
    assert_eq!(cfg.thresholds.len(), 2);

    // Remove the first (index 0).
    let cfg2 = client.remove_alert_threshold(&staker, &asset, &0);
    assert_eq!(cfg2.thresholds.len(), 1);
    assert_eq!(cfg2.thresholds.get(0).unwrap().trigger_value, SCALE / 100);
}

#[test]
fn disable_and_reenable_alerts() {
    let (env, client) = setup();
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    let t = make_threshold(&env, AlertKind::BalanceDrop, 100, AlertSeverity::Warning, "floor");
    client.set_alert_config(&staker, &asset, &vec![&env, t], &true);

    let off = client.set_alerts_enabled(&staker, &asset, &false);
    assert!(!off.alerts_enabled);

    let on = client.set_alerts_enabled(&staker, &asset, &true);
    assert!(on.alerts_enabled);
}

#[test]
fn no_config_returns_none() {
    let (env, client) = setup();
    let staker = Address::generate(&env);
    let asset = symbol_short!("USDC");
    assert!(client.get_alert_config(&staker, &asset).is_none());
}

// ---------------------------------------------------------------------------
// Alert threshold evaluation tests
// ---------------------------------------------------------------------------

#[test]
fn balance_drop_alert_fires_when_below_floor() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    // Threshold: balance < 500 → Warning.
    let t = make_threshold(&env, AlertKind::BalanceDrop, 500, AlertSeverity::Warning, "low balance");
    client.set_alert_config(&staker, &asset, &vec![&env, t], &true);

    // Balance is 300 — below the floor.
    let fired = client.check_alerts(&staker, &asset, &300, &(SCALE / 20), &0);
    assert_eq!(fired, 1);

    // Balance is 600 — above the floor; no alert.
    let not_fired = client.check_alerts(&staker, &asset, &600, &(SCALE / 20), &0);
    assert_eq!(not_fired, 0);
}

#[test]
fn yield_underperformance_alert_fires_when_apr_too_low() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    // Threshold: APR < 3% → Warning.
    let min_apr = 3 * SCALE / 100;
    let t = make_threshold(&env, AlertKind::YieldUnderperformance, min_apr, AlertSeverity::Warning, "low apr");
    client.set_alert_config(&staker, &asset, &vec![&env, t], &true);

    // APR 2% — below minimum.
    let fired = client.check_alerts(&staker, &asset, &1_000, &(2 * SCALE / 100), &0);
    assert_eq!(fired, 1);

    // APR 5% — above minimum.
    let not_fired = client.check_alerts(&staker, &asset, &1_000, &(5 * SCALE / 100), &0);
    assert_eq!(not_fired, 0);
}

#[test]
fn upcoming_unlock_alert_fires_within_window() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    // Threshold: unlock within 7 days → Warning.
    let window = 7 * SECONDS_PER_DAY as i128;
    let t = make_threshold(&env, AlertKind::UpcomingUnlock, window, AlertSeverity::Warning, "unlock soon");
    client.set_alert_config(&staker, &asset, &vec![&env, t], &true);

    // Unlock in 3 days — within the window.
    let unlock_ts: u64 = 1_000 + 3 * SECONDS_PER_DAY;
    let fired = client.check_alerts(&staker, &asset, &1_000, &(SCALE / 20), &unlock_ts);
    assert_eq!(fired, 1);

    // Unlock in 10 days — outside the window.
    let unlock_far: u64 = 1_000 + 10 * SECONDS_PER_DAY;
    let not_fired = client.check_alerts(&staker, &asset, &1_000, &(SCALE / 20), &unlock_far);
    assert_eq!(not_fired, 0);
}

#[test]
fn upcoming_unlock_zero_ts_does_not_fire() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    let t = make_threshold(&env, AlertKind::UpcomingUnlock, 7 * SECONDS_PER_DAY as i128, AlertSeverity::Info, "unlock");
    client.set_alert_config(&staker, &asset, &vec![&env, t], &true);

    // unlock_ts == 0 means no lock-up; threshold must not fire.
    let fired = client.check_alerts(&staker, &asset, &1_000, &(SCALE / 20), &0);
    assert_eq!(fired, 0);
}

#[test]
fn custom_alert_fires_on_exact_match() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    // Custom threshold fires only when observed == trigger_value (0 == 0).
    let t = make_threshold(&env, AlertKind::Custom, 0, AlertSeverity::Info, "custom trigger");
    client.set_alert_config(&staker, &asset, &vec![&env, t], &true);

    // observed is always 0 for Custom; trigger_value is also 0 → fires.
    let fired = client.check_alerts(&staker, &asset, &1_000, &(SCALE / 20), &0);
    assert_eq!(fired, 1);
}

#[test]
fn disabled_config_does_not_fire() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    let t = make_threshold(&env, AlertKind::BalanceDrop, 1_000, AlertSeverity::Critical, "floor");
    client.set_alert_config(&staker, &asset, &vec![&env, t], &false);

    // alerts_enabled is false — nothing fires even though balance < threshold.
    let fired = client.check_alerts(&staker, &asset, &50, &(SCALE / 20), &0);
    assert_eq!(fired, 0);
}

#[test]
fn disabled_threshold_does_not_fire() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    let mut t = make_threshold(&env, AlertKind::BalanceDrop, 1_000, AlertSeverity::Critical, "floor");
    t.enabled = false;
    client.set_alert_config(&staker, &asset, &vec![&env, t], &true);

    // Threshold individually disabled — no alert.
    let fired = client.check_alerts(&staker, &asset, &50, &(SCALE / 20), &0);
    assert_eq!(fired, 0);
}

#[test]
fn multiple_thresholds_all_fire() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    let t1 = make_threshold(&env, AlertKind::BalanceDrop, 500, AlertSeverity::Warning, "balance");
    let t2 = make_threshold(&env, AlertKind::YieldUnderperformance, SCALE / 10, AlertSeverity::Critical, "yield");
    client.set_alert_config(&staker, &asset, &vec![&env, t1, t2], &true);

    // Both conditions breached: balance 100 < 500, APR 1% < 10%.
    let fired = client.check_alerts(&staker, &asset, &100, &(SCALE / 100), &0);
    assert_eq!(fired, 2);
}

// ---------------------------------------------------------------------------
// Alert severity levels
// ---------------------------------------------------------------------------

#[test]
fn severity_ordering_is_correct() {
    assert!(AlertSeverity::Info < AlertSeverity::Warning);
    assert!(AlertSeverity::Warning < AlertSeverity::Critical);
}

#[test]
fn alert_history_records_correct_severity() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    let t = make_threshold(&env, AlertKind::BalanceDrop, 500, AlertSeverity::Critical, "critical floor");
    client.set_alert_config(&staker, &asset, &vec![&env, t], &true);
    client.check_alerts(&staker, &asset, &100, &(SCALE / 20), &0);

    let history = client.alert_history(&staker, &asset);
    assert_eq!(history.len(), 1);
    let entry = history.get(0).unwrap();
    assert_eq!(entry.severity, AlertSeverity::Critical);
    assert_eq!(entry.kind, AlertKind::BalanceDrop);
}

// ---------------------------------------------------------------------------
// Alert history and acknowledgment tests
// ---------------------------------------------------------------------------

#[test]
fn alert_history_appends_on_each_fire() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    let t = make_threshold(&env, AlertKind::BalanceDrop, 500, AlertSeverity::Warning, "floor");
    client.set_alert_config(&staker, &asset, &vec![&env, t], &true);

    // Fire twice at different timestamps.
    client.check_alerts(&staker, &asset, &100, &(SCALE / 20), &0);
    env.ledger().set_timestamp(2_000);
    client.check_alerts(&staker, &asset, &200, &(SCALE / 20), &0);

    let history = client.alert_history(&staker, &asset);
    assert_eq!(history.len(), 2);
    assert_eq!(history.get(0).unwrap().fired_at, 1_000);
    assert_eq!(history.get(1).unwrap().fired_at, 2_000);
}

#[test]
fn acknowledge_alert_sets_flag() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    let t = make_threshold(&env, AlertKind::BalanceDrop, 500, AlertSeverity::Info, "floor");
    client.set_alert_config(&staker, &asset, &vec![&env, t], &true);
    client.check_alerts(&staker, &asset, &100, &(SCALE / 20), &0);

    // Not yet acknowledged.
    assert!(!client.alert_history(&staker, &asset).get(0).unwrap().acknowledged);

    client.acknowledge_alert(&staker, &asset, &0);
    assert!(client.alert_history(&staker, &asset).get(0).unwrap().acknowledged);
}

#[test]
fn pending_alerts_excludes_acknowledged() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    let t = make_threshold(&env, AlertKind::BalanceDrop, 500, AlertSeverity::Warning, "floor");
    client.set_alert_config(&staker, &asset, &vec![&env, t], &true);

    // Fire twice.
    client.check_alerts(&staker, &asset, &100, &(SCALE / 20), &0);
    env.ledger().set_timestamp(2_000);
    client.check_alerts(&staker, &asset, &200, &(SCALE / 20), &0);

    assert_eq!(client.pending_alerts(&staker, &asset).len(), 2);

    // Acknowledge the first.
    client.acknowledge_alert(&staker, &asset, &0);
    assert_eq!(client.pending_alerts(&staker, &asset).len(), 1);

    // Acknowledge the second.
    client.acknowledge_alert(&staker, &asset, &1);
    assert_eq!(client.pending_alerts(&staker, &asset).len(), 0);

    // Full history is still intact.
    assert_eq!(client.alert_history(&staker, &asset).len(), 2);
}

#[test]
fn alert_history_is_empty_before_any_fire() {
    let (env, client) = setup();
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    assert_eq!(client.alert_history(&staker, &asset).len(), 0);
    assert_eq!(client.pending_alerts(&staker, &asset).len(), 0);
}

#[test]
fn check_alerts_returns_zero_with_no_config() {
    let (env, client) = setup();
    env.ledger().set_timestamp(1_000);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");
    // No config registered — should return 0, not panic.
    let fired = client.check_alerts(&staker, &asset, &100, &(SCALE / 20), &0);
    assert_eq!(fired, 0);
}

// ---------------------------------------------------------------------------
// Yield engine contract tests
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
fn rate_change_is_time_weighted() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.open_yield_position(
        &staker, &asset, &SCALE, &(4 * SCALE / 100), &CompoundingMode::Continuous,
    );
    env.ledger().set_timestamp(SECONDS_PER_YEAR / 2);
    client.set_yield_rate(&staker, &asset, &(8 * SCALE / 100));
    env.ledger().set_timestamp(SECONDS_PER_YEAR);
    let rec = client.accrue_yield(&staker, &asset);

    let seg1 = 20_201_340_026_755_810i128;
    let seg2 = 40_810_774_192_388_960i128;
    approx(rec.accrued_yield, seg1 + seg2, 10_000_000_000);
}

#[test]
fn history_is_complete_and_queryable() {
    let (env, client) = setup();
    env.ledger().set_timestamp(0);
    let staker = Address::generate(&env);
    let asset = symbol_short!("XLM");

    client.open_yield_position(&staker, &asset, &SCALE, &(SCALE / 20), &CompoundingMode::Daily);
    env.ledger().set_timestamp(30 * SECONDS_PER_DAY);
    client.accrue_yield(&staker, &asset);
    env.ledger().set_timestamp(60 * SECONDS_PER_DAY);
    client.accrue_yield(&staker, &asset);
    env.ledger().set_timestamp(90 * SECONDS_PER_DAY);
    let rec = client.accrue_yield(&staker, &asset);

    let history = client.yield_history(&staker, &asset);
    assert_eq!(history.len(), 3);
    let last = history.get(2).unwrap();
    assert_eq!(last.cumulative_yield, rec.accrued_yield);
    assert_eq!(last.timestamp, 90 * SECONDS_PER_DAY);
    assert_eq!(history.get(0).unwrap().period_seconds, 30 * SECONDS_PER_DAY);
}

#[test]
fn thirty_day_projection_within_one_percent() {
    let (_env, client) = setup();
    let horizon = 30 * SECONDS_PER_DAY;
    let proj = client.project_yield(&SCALE, &(SCALE / 10), &CompoundingMode::Continuous, &horizon);
    let expected = 8_253_048_640_000_000i128;
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

    client.schedule_distribution(&staker, &asset, &500, &2_000, &0);
    assert_eq!(client.process_distribution(&staker, &asset), 0);

    env.ledger().set_timestamp(2_000);
    assert_eq!(client.process_distribution(&staker, &asset), 500);

    env.ledger().set_timestamp(3_000);
    assert_eq!(client.process_distribution(&staker, &asset), 0);
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
fn claim_yield_resets_unclaimed() {
    let (env, client) = setup();
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
