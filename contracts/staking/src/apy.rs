//! APY / APR conversion utilities.
//!
//! **APR** (annual percentage *rate*) is the simple annualized rate, ignoring
//! compounding. **APY** (annual percentage *yield*) is the effective annual
//! return once compounding is taken into account. This module converts between
//! the two for each supported [`Compounding`] model.
//!
//! # Formulas
//!
//! For a nominal APR `r`:
//! - **Daily:** `APY = (1 + r/365)^365 - 1`, and inversely
//!   `APR = 365 * ((1 + APY)^(1/365) - 1)`.
//! - **Continuous:** `APY = e^r - 1`, and inversely `APR = ln(1 + APY)`.
//!
//! All values are fixed-point fractions (e.g. `0.05 * SCALE` is 5%).

use crate::compounding::{Compounding, CompoundingStrategy};
use crate::fixed_point::{self as fp, MathError, ONE, SECONDS_PER_YEAR};

/// Converts between APR and APY for a given compounding model.
pub struct APYCalculator;

impl APYCalculator {
    /// Convert a nominal `apr` into the effective APY under `compounding`.
    ///
    /// This is exactly the one-year growth factor minus one, so it reuses the
    /// compounding strategy directly and stays consistent with
    /// [`crate::compounding::YieldCalculator`].
    pub fn apr_to_apy(apr: i128, compounding: Compounding) -> Result<i128, MathError> {
        let factor = compounding.growth_factor(apr, SECONDS_PER_YEAR)?;
        factor.checked_sub(ONE).ok_or(MathError::Overflow)
    }

    /// Convert an effective `apy` back into the nominal APR under `compounding`.
    ///
    /// - Continuous: `APR = ln(1 + APY)`.
    /// - Daily: `APR = 365 * ((1 + APY)^(1/365) - 1)`, computed via the
    ///   365th root of `(1 + APY)`.
    pub fn apy_to_apr(apy: i128, compounding: Compounding) -> Result<i128, MathError> {
        let one_plus = ONE.checked_add(apy).ok_or(MathError::Overflow)?;
        match compounding {
            Compounding::Continuous => ln(one_plus),
            Compounding::Daily => {
                // periodic rate = (1 + APY)^(1/365) - 1; APR = 365 * periodic.
                let root = nth_root(one_plus, 365)?;
                let periodic = root.checked_sub(ONE).ok_or(MathError::Overflow)?;
                fp::mul(periodic, 365 * ONE)
            }
        }
    }
}

/// Natural logarithm of a fixed-point value `x > 0`.
///
/// Uses the identity `ln(x) = 2 * atanh((x-1)/(x+1))` with the series
/// `atanh(y) = y + y^3/3 + y^5/5 + ...`, which converges rapidly for `x` near 1
/// (the regime for `1 + APY`). Range reduction by factors of `e` keeps the
/// argument close to 1 for larger inputs.
pub fn ln(x: i128) -> Result<i128, MathError> {
    if x <= 0 {
        return Err(MathError::NegativeInput);
    }
    if x == ONE {
        return Ok(0);
    }

    // Range-reduce toward 1 by dividing/multiplying by e and counting.
    let e = fp::exp(ONE)?; // e in fixed-point
    let inv_e = fp::div(ONE, e)?;
    let mut val = x;
    let mut k: i128 = 0;
    // Bring val into [1/e, e] roughly, so the atanh series converges fast.
    while val > e {
        val = fp::mul(val, inv_e)?;
        k += 1;
    }
    while val < inv_e {
        val = fp::mul(val, e)?;
        k -= 1;
    }

    // y = (val - 1) / (val + 1)
    let num = val.checked_sub(ONE).ok_or(MathError::Overflow)?;
    let den = val.checked_add(ONE).ok_or(MathError::Overflow)?;
    let y = fp::div(num, den)?;
    let y2 = fp::mul(y, y)?;

    // atanh series: sum of y^(2n+1)/(2n+1)
    let mut term = y;
    let mut sum = y;
    for n in 1..=30i128 {
        term = fp::mul(term, y2)?;
        let add = term / (2 * n + 1);
        sum = sum.checked_add(add).ok_or(MathError::Overflow)?;
        if add == 0 {
            break;
        }
    }
    let ln_reduced = fp::mul(2 * ONE, sum)?;

    // ln(x) = k + ln_reduced (since each reduction step multiplied by e^±1).
    (k.checked_mul(ONE).ok_or(MathError::Overflow)?)
        .checked_add(ln_reduced)
        .ok_or(MathError::Overflow)
}

/// The `n`-th root of a fixed-point value `x >= 0` for `n >= 1`.
///
/// Computed as `exp(ln(x) / n)`. Returns [`ONE`] for `x == ONE` or `n == 1`
/// short-circuits to `x`.
pub fn nth_root(x: i128, n: u64) -> Result<i128, MathError> {
    if n == 0 {
        return Err(MathError::DivideByZero);
    }
    if n == 1 || x == ONE {
        return Ok(x);
    }
    if x <= 0 {
        return Err(MathError::NegativeInput);
    }
    let l = ln(x)?;
    let scaled = l / (n as i128);
    // scaled may be negative if x < 1; exp only supports non-negative inputs,
    // but for APY math x = 1 + APY >= 1, so ln(x) >= 0 and scaled >= 0.
    fp::exp(scaled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed_point::SCALE;

    fn approx(a: i128, b: i128, tol: i128) {
        let diff = (a - b).abs();
        assert!(diff <= tol, "expected {} ~= {} within {}, diff {}", a, b, tol, diff);
    }

    #[test]
    fn ln_of_e_is_one() {
        let e = fp::exp(ONE).unwrap();
        approx(ln(e).unwrap(), ONE, 1_000_000);
    }

    #[test]
    fn ln_of_one_is_zero() {
        assert_eq!(ln(ONE).unwrap(), 0);
    }

    #[test]
    fn ln_known_value() {
        // ln(2) = 0.6931471805599453
        approx(ln(2 * ONE).unwrap(), 693_147_180_559_945_309, 10_000_000);
    }

    #[test]
    fn daily_apr_to_apy_known() {
        // 5% APR daily -> APY = (1+0.05/365)^365 - 1 = 0.05126749650...
        let apy = APYCalculator::apr_to_apy(SCALE / 20, Compounding::Daily).unwrap();
        approx(apy, 51_267_496_505_408_400, 100_000_000_000);
    }

    #[test]
    fn continuous_apr_to_apy_known() {
        // 5% APR continuous -> APY = e^0.05 - 1 = 0.05127109637...
        let apy = APYCalculator::apr_to_apy(SCALE / 20, Compounding::Continuous).unwrap();
        approx(apy, 51_271_096_376_024_040, 100_000_000_000);
    }

    #[test]
    fn roundtrip_continuous() {
        // apr -> apy -> apr should return the original within tolerance.
        let apr = SCALE / 10; // 10%
        let apy = APYCalculator::apr_to_apy(apr, Compounding::Continuous).unwrap();
        let back = APYCalculator::apy_to_apr(apy, Compounding::Continuous).unwrap();
        approx(back, apr, 10_000_000);
    }

    #[test]
    fn roundtrip_daily() {
        let apr = SCALE / 10; // 10%
        let apy = APYCalculator::apr_to_apy(apr, Compounding::Daily).unwrap();
        let back = APYCalculator::apy_to_apr(apy, Compounding::Daily).unwrap();
        // 0.01% accuracy target -> tol ~1e14 on 1e18 scale
        approx(back, apr, 100_000_000_000_000);
    }
}
