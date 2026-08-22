//! Deterministic fixed-point performance metric calculations.
//!
//! This module provides pure-function implementations of industry-standard
//! portfolio performance formulas using the same 1e18 fixed-point scale
//! as the staking contract. All validators compute identical results.
//!
//! # Conventions
//!
//! - Returns are **decimal fractions** (0.10 = +10%).
//! - Ratios like Sharpe/Sortino are dimensionless decimal values.
//! - Maximum drawdown is reported as a **positive** fraction (0.25 = 25% drop).
//! - All inputs and outputs use [`SCALE`] (1e18) fixed-point representation.

use soroban_sdk::Vec;

use crate::records::{ONE, SCALE};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors arising from performance calculations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfError {
    /// A division by zero was attempted.
    DivideByZero,
    /// Insufficient data points for the requested calculation (e.g. fewer than
    /// two snapshots for drawdown).
    InsufficientData,
    /// Computation overflowed the i128 range.
    Overflow,
}

// ---------------------------------------------------------------------------
// Fixed-point arithmetic helpers (self-contained for this module)
// ---------------------------------------------------------------------------

/// Multiply two fixed-point values: `(a * b) / SCALE`.
pub fn fp_mul(a: i128, b: i128) -> Result<i128, PerfError> {
    fp_mul_div(a, b, SCALE)
}

/// Divide two fixed-point values: `(a * SCALE) / b`.
pub fn fp_div(a: i128, b: i128) -> Result<i128, PerfError> {
    if b == 0 {
        return Err(PerfError::DivideByZero);
    }
    fp_mul_div(a, SCALE, b)
}

/// Compute `a * b / denom` with a 256-bit intermediate to avoid overflow.
pub fn fp_mul_div(a: i128, b: i128, denom: i128) -> Result<i128, PerfError> {
    if denom == 0 {
        return Err(PerfError::DivideByZero);
    }

    let neg = (a < 0) ^ (b < 0) ^ (denom < 0);
    let a_abs = a.unsigned_abs();
    let b_abs = b.unsigned_abs();
    let d = denom.unsigned_abs();

    // 256-bit product via schoolbook
    let a_lo = a_abs & 0xFFFF_FFFF_FFFF_FFFF;
    let a_hi = a_abs >> 64;
    let b_lo = b_abs & 0xFFFF_FFFF_FFFF_FFFF;
    let b_hi = b_abs >> 64;

    let ll = a_lo * b_lo;
    let lh = a_lo * b_hi;
    let hl = a_hi * b_lo;
    let hh = a_hi * b_hi;

    let mut lo = ll;
    let mut hi = hh;

    let (m1, c1) = lo.overflowing_add(lh << 64);
    lo = m1;
    hi = hi.wrapping_add((lh >> 64).wrapping_add(c1 as u128));

    let (m2, c2) = lo.overflowing_add(hl << 64);
    lo = m2;
    hi = hi.wrapping_add((hl >> 64).wrapping_add(c2 as u128));

    // Binary long division of 256-bit / 128-bit
    if hi == 0 {
        let q = lo / d;
        if q > i128::MAX as u128 {
            return Err(PerfError::Overflow);
        }
        return Ok(if neg { -(q as i128) } else { q as i128 });
    }

    // Full 256-bit division
    let mut quotient_hi: u128 = 0;
    let mut quotient_lo: u128 = 0;
    let mut rem: u128 = 0;

    for i in (0..256u32).rev() {
        let bit = if i >= 128 {
            (hi >> (i - 128)) & 1
        } else {
            (lo >> i) & 1
        };

        let overflow_top = rem >> 127;
        rem = (rem << 1) | bit;

        if overflow_top == 1 || rem >= d {
            rem = rem.wrapping_sub(d);
            if i >= 128 {
                quotient_hi |= 1u128 << (i - 128);
            } else {
                quotient_lo |= 1u128 << i;
            }
        }
    }

    if quotient_hi != 0 {
        return Err(PerfError::Overflow);
    }
    Ok(if neg {
        -(quotient_lo as i128)
    } else {
        quotient_lo as i128
    })
}

/// Fixed-point square root via Newton's method.
///
/// For a fixed-point input `x` (where real value = x / SCALE), returns
/// `sqrt(real_value) * SCALE` in fixed-point.
///
/// Newton's method in fixed-point space:
///   result_{k+1} = (result_k + fp_div(x, result_k)) / 2
///
/// Converges because for any positive fixed-point x, the iteration
/// oscillates around sqrt(x) and halves the error each step.
pub fn fp_sqrt(x: i128) -> Result<i128, PerfError> {
    if x < 0 {
        return Err(PerfError::Overflow);
    }
    if x == 0 {
        return Ok(0);
    }

    // Initial guess: x / 2 (always >= sqrt(x) for x >= 4, close enough otherwise)
    let mut guess = x / 2;
    if guess == 0 {
        guess = 1;
    }

    // Newton's method: guess' = (guess + x/guess) / 2
    // Using fp_div for the division (fixed-point / fixed-point = fixed-point)
    for _ in 0..128 {
        let q = fp_div(x, guess).map_err(|_| PerfError::Overflow)?;
        let new_guess = (guess + q) / 2;
        if new_guess >= guess {
            break;
        }
        guess = new_guess;
    }

    Ok(guess)
}

// ---------------------------------------------------------------------------
// Sharpe Ratio
// ---------------------------------------------------------------------------

/// Compute the annualized Sharpe ratio from a series of periodic returns.
///
/// `risk_free_rate` is the annualized risk-free rate in decimal fixed-point
/// (e.g. 0.04 for 4%).
///
/// `periodic_returns` is a slice of periodic (e.g. daily) returns in
/// decimal fixed-point. Must contain at least 2 entries.
///
/// `periods_per_year` is the number of periods in a year (e.g. 365 for daily).
///
/// Formula: `(mean(returns) - rf/periods) / stddev(returns) * sqrt(periods_per_year)`
pub fn sharpe_ratio(
    periodic_returns: &Vec<i128>,
    risk_free_rate: i128,
    periods_per_year: u64,
) -> Result<i128, PerfError> {
    let n = periodic_returns.len();
    if n < 2 {
        return Err(PerfError::InsufficientData);
    }
    if periods_per_year == 0 {
        return Err(PerfError::DivideByZero);
    }

    let n_fp = n as i128;

    // Mean return: sum is already in fixed-point, divide by count.
    let mut sum = 0i128;
    for i in 0..n {
        let r = periodic_returns.get(i).unwrap();
        sum = sum.checked_add(r).ok_or(PerfError::Overflow)?;
    }
    // mean = sum / n (both fixed-point, result stays fixed-point)
    let mean = sum / n_fp;

    // Risk-free return per period
    let rf_per_period = fp_div(risk_free_rate, (periods_per_year as i128) * ONE)?;

    // Excess mean
    let excess_mean = mean - rf_per_period;

    // Standard deviation (sample)
    let mut sum_sq_diff = 0i128;
    for i in 0..n {
        let r = periodic_returns.get(i).unwrap();
        let diff = r - mean;
        let diff_sq = fp_mul(diff, diff)?;
        sum_sq_diff = sum_sq_diff
            .checked_add(diff_sq)
            .ok_or(PerfError::Overflow)?;
    }
    // variance = sum_sq_diff / (n-1)
    // sum_sq_diff is in SCALE^2; dividing by integer (n-1) keeps it in SCALE^2
    let variance = sum_sq_diff / (n_fp - 1);
    let stddev = fp_sqrt(variance)?;

    if stddev == 0 {
        return Ok(0);
    }

    // Sharpe = excess_mean / stddev * sqrt(periods_per_year)
    let periods_fp = (periods_per_year as i128) * ONE;
    let sqrt_periods = fp_sqrt(periods_fp)?;
    let sharpe = fp_mul(fp_div(excess_mean, stddev)?, sqrt_periods)?;

    Ok(sharpe)
}

// ---------------------------------------------------------------------------
// Sortino Ratio
// ---------------------------------------------------------------------------

/// Compute the annualized Sortino ratio from a series of periodic returns.
///
/// Like [`sharpe_ratio`] but uses downside deviation (standard deviation of
/// only negative excess returns) instead of total standard deviation.
///
/// Formula: `(mean(returns) - rf/periods) / downside_deviation * sqrt(periods_per_year)`
pub fn sortino_ratio(
    periodic_returns: &Vec<i128>,
    risk_free_rate: i128,
    periods_per_year: u64,
) -> Result<i128, PerfError> {
    let n = periodic_returns.len();
    if n < 2 {
        return Err(PerfError::InsufficientData);
    }
    if periods_per_year == 0 {
        return Err(PerfError::DivideByZero);
    }

    let n_fp = n as i128;

    // Mean return
    let mut sum = 0i128;
    for i in 0..n {
        let r = periodic_returns.get(i).unwrap();
        sum = sum.checked_add(r).ok_or(PerfError::Overflow)?;
    }
    let mean = sum / n_fp;

    // Risk-free return per period
    let rf_per_period = fp_div(risk_free_rate, (periods_per_year as i128) * ONE)?;

    // Excess mean
    let excess_mean = mean - rf_per_period;

    // Downside deviation: sqrt(sum of (min(0, excess))^2 / (n-1))
    let mut sum_sq_down = 0i128;
    for i in 0..n {
        let r = periodic_returns.get(i).unwrap();
        let excess = r - rf_per_period;
        if excess < 0 {
            let sq = fp_mul(excess, excess)?;
            sum_sq_down = sum_sq_down.checked_add(sq).ok_or(PerfError::Overflow)?;
        }
    }

    // downside_var is in SCALE^2; divide by integer (n-1)
    let downside_var = sum_sq_down / (n_fp - 1);
    let downside_dev = fp_sqrt(downside_var)?;

    if downside_dev == 0 {
        // No downside risk: sortino is effectively infinite; return a large value
        // or 0 if excess is also 0.
        if excess_mean <= 0 {
            return Ok(0);
        }
        return Ok(i128::MAX / 2); // represent "very large"
    }

    let periods_fp = (periods_per_year as i128) * ONE;
    let sqrt_periods = fp_sqrt(periods_fp)?;
    let sortino = fp_mul(fp_div(excess_mean, downside_dev)?, sqrt_periods)?;

    Ok(sortino)
}

// ---------------------------------------------------------------------------
// Maximum Drawdown
// ---------------------------------------------------------------------------

/// Compute the maximum drawdown from a series of portfolio valuations.
///
/// `valuations` is a slice of total portfolio values (fixed-point) in
/// chronological order. Must contain at least 2 entries.
///
/// Returns the maximum drawdown as a **positive** fraction (e.g. 0.25 = 25%
/// drawdown). Returns 0 if the portfolio never declined from a peak.
pub fn max_drawdown(valuations: &Vec<i128>) -> Result<i128, PerfError> {
    let n = valuations.len();
    if n < 2 {
        return Err(PerfError::InsufficientData);
    }

    let mut peak = valuations.get(0).unwrap();
    let mut max_dd = 0i128;

    for i in 1..n {
        let val = valuations.get(i).unwrap();
        if val > peak {
            peak = val;
        }
        if peak > 0 {
            let drawdown = fp_div(peak - val, peak)?;
            if drawdown > max_dd {
                max_dd = drawdown;
            }
        }
    }

    Ok(max_dd)
}

// ---------------------------------------------------------------------------
// Time-Weighted Return (TWR)
// ---------------------------------------------------------------------------

/// Compute the time-weighted return from a series of portfolio valuations.
///
/// `valuations` is a slice of total portfolio values (fixed-point) in
/// chronological order. Must contain at least 2 entries.
///
/// TWR eliminates the distorting effect of cash flows by chaining
/// sub-period returns:
///
/// `TWR = Π(1 + r_i) - 1` where `r_i = V_i / V_{i-1} - 1`
///
/// Returns the cumulative TWR as a decimal fraction in fixed-point
/// (e.g. 0.10 = +10%).
pub fn time_weighted_return(valuations: &Vec<i128>) -> Result<i128, PerfError> {
    let n = valuations.len();
    if n < 2 {
        return Err(PerfError::InsufficientData);
    }

    // Product of (V_i / V_{i-1})
    let mut product = SCALE; // start at 1.0

    for i in 1..n {
        let prev = valuations.get(i - 1).unwrap();
        let curr = valuations.get(i).unwrap();
        if prev == 0 {
            return Err(PerfError::DivideByZero);
        }
        // ratio = curr / prev (both fixed-point)
        let ratio = fp_div(curr, prev)?;
        product = fp_mul(product, ratio)?;
    }

    // TWR = product - 1
    Ok(product - SCALE)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{vec as sdk_vec, Env};

    fn approx(a: i128, b: i128, tol: i128) {
        let diff = (a - b).abs();
        assert!(
            diff <= tol,
            "expected {} ~= {} within {}, diff was {}",
            a,
            b,
            tol,
            diff
        );
    }

    #[test]
    fn test_fp_mul_basic() {
        // 2.0 * 3.0 = 6.0
        let a = 2 * SCALE;
        let b = 3 * SCALE;
        assert_eq!(fp_mul(a, b).unwrap(), 6 * SCALE);
    }

    #[test]
    fn test_fp_mul_fractional() {
        // 0.5 * 0.5 = 0.25
        let half = SCALE / 2;
        approx(fp_mul(half, half).unwrap(), SCALE / 4, 1);
    }

    #[test]
    fn test_fp_div_basic() {
        // 6.0 / 3.0 = 2.0
        assert_eq!(fp_div(6 * SCALE, 3 * SCALE).unwrap(), 2 * SCALE);
    }

    #[test]
    fn test_fp_div_by_zero() {
        assert_eq!(fp_div(SCALE, 0), Err(PerfError::DivideByZero));
    }

    #[test]
    fn test_fp_sqrt() {
        // sqrt(4.0) = 2.0
        approx(fp_sqrt(4 * SCALE).unwrap(), 2 * SCALE, 100);
        // sqrt(1.0) = 1.0
        approx(fp_sqrt(SCALE).unwrap(), SCALE, 100);
        // sqrt(0) = 0
        assert_eq!(fp_sqrt(0).unwrap(), 0);
    }

    #[test]
    fn test_sharpe_ratio_basic() {
        let env = Env::default();
        // 5% risk-free rate
        let rf = SCALE / 20;
        // Periods per year: 365 (daily)
        let periods = 365u64;

        // All positive returns (above risk-free per period)
        let returns = sdk_vec![
            &env,
            SCALE / 100, // 1%
            SCALE / 50,  // 2%
            SCALE / 100, // 1%
            SCALE / 200, // 0.5%
            SCALE / 80,  // 1.25%
            SCALE / 100, // 1%
            SCALE / 120, // ~0.83%
            SCALE / 90,  // ~1.11%
            SCALE / 110, // ~0.91%
            SCALE / 100, // 1%
        ];

        let sharpe = sharpe_ratio(&returns, rf, periods).unwrap();
        // Sharpe should be positive since all returns exceed the daily risk-free rate
        assert!(sharpe > 0, "Sharpe should be positive, got {}", sharpe);
    }

    #[test]
    fn test_sharpe_ratio_insufficient_data() {
        let env = Env::default();
        let returns = sdk_vec![&env, SCALE / 100];
        assert_eq!(
            sharpe_ratio(&returns, SCALE / 20, 365),
            Err(PerfError::InsufficientData)
        );
    }

    #[test]
    fn test_sortino_ratio_basic() {
        let env = Env::default();
        let rf = SCALE / 20; // 5% annual
        let periods = 365u64;

        // Mix of positive and negative returns
        let returns = sdk_vec![
            &env,
            SCALE / 100,    // 1%
            -(SCALE / 100), // -1%
            SCALE / 50,     // 2%
            -(SCALE / 200), // -0.5%
            SCALE / 100,    // 1%
            SCALE / 80,     // 1.25%
            -(SCALE / 150), // -0.67%
            SCALE / 100,    // 1%
            SCALE / 90,     // ~1.11%
            SCALE / 110,    // ~0.91%
        ];

        let sortino = sortino_ratio(&returns, rf, periods).unwrap();
        // Sortino should exist (not error)
        assert_ne!(sortino, 0, "Sortino should be non-zero");
    }

    #[test]
    fn test_max_drawdown_basic() {
        let env = Env::default();
        let vals = sdk_vec![
            &env,
            100 * SCALE, // peak
            90 * SCALE,  // -10%
            95 * SCALE,  // recovery
            80 * SCALE,  // -15.79% from peak of 95
            85 * SCALE,  // recovery
        ];

        // Max drawdown: from 100 to 80 = 20%
        let dd = max_drawdown(&vals).unwrap();
        approx(dd, SCALE / 5, SCALE / 1000); // ~0.20
    }

    #[test]
    fn test_max_drawdown_no_drawdown() {
        let env = Env::default();
        let vals = sdk_vec![&env, 100 * SCALE, 110 * SCALE, 120 * SCALE, 130 * SCALE,];
        assert_eq!(max_drawdown(&vals).unwrap(), 0);
    }

    #[test]
    fn test_max_drawdown_insufficient_data() {
        let env = Env::default();
        let vals = sdk_vec![&env, 100 * SCALE];
        assert_eq!(max_drawdown(&vals), Err(PerfError::InsufficientData));
    }

    #[test]
    fn test_time_weighted_return_basic() {
        let env = Env::default();
        // Portfolio: 100 -> 110 -> 105 -> 115.5
        // Period returns: 10%, -4.55%, 10%
        // TWR = (1.10 * 0.9545... * 1.10) - 1 ≈ 0.155
        let vals = sdk_vec![
            &env,
            100 * SCALE,
            110 * SCALE,
            105 * SCALE,
            115_500_000_000_000_000_000, // 115.5
        ];

        let twr = time_weighted_return(&vals).unwrap();
        // Expected: (110/100) * (105/110) * (115.5/105) - 1 = 115.5/100 - 1 = 0.155
        approx(twr, 155 * SCALE / 1000, SCALE / 1000);
    }

    #[test]
    fn test_time_weighted_return_insufficient_data() {
        let env = Env::default();
        let vals = sdk_vec![&env, 100 * SCALE];
        assert_eq!(
            time_weighted_return(&vals),
            Err(PerfError::InsufficientData)
        );
    }
}
