//! Compounding strategies and the core yield calculator.
//!
//! This module implements the two compounding models required by the yield
//! engine — **daily** and **continuous** — behind a common [`CompoundingStrategy`]
//! trait, and a [`YieldCalculator`] that applies a chosen strategy to a staked
//! principal over an arbitrary duration.
//!
//! # Financial model
//!
//! Given an annual percentage *rate* `r` (APR, as a fraction), a principal `P`,
//! and a duration `t` (in years):
//!
//! - **Daily compounding** grows the principal by a factor of
//!   `(1 + r/365)^(365 * t)`. For a whole number of days `d`, this is
//!   `(1 + r/365)^d`. Partial days are handled by a final fractional period.
//! - **Continuous compounding** grows the principal by `e^(r * t)`.
//!
//! The *yield* (interest earned) is `P * (factor - 1)`.
//!
//! All computation is in fixed-point (see [`crate::fixed_point`]); durations are
//! supplied in seconds and converted internally to a fixed-point year fraction.

use crate::fixed_point::{
    self as fp, MathError, DAYS_PER_YEAR, ONE, SECONDS_PER_DAY, SECONDS_PER_YEAR,
};

/// The compounding model applied to a staked position.
///
/// This is a plain enum (rather than trait objects) because Soroban's `no_std`
/// environment favors value types that can be stored and passed by copy. The
/// [`CompoundingStrategy`] trait below is implemented for it to expose the
/// growth-factor computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compounding {
    /// Interest is compounded once per day (365 periods per year).
    Daily,
    /// Interest is compounded continuously (`e^(rt)`).
    Continuous,
}

/// Behavior shared by all compounding strategies: computing the growth factor
/// applied to a principal over a given duration at a given annual rate.
pub trait CompoundingStrategy {
    /// Compute the fixed-point growth factor for annual rate `apr` applied over
    /// `duration_seconds`.
    ///
    /// The returned factor `f` is in fixed-point such that
    /// `new_principal = principal * f`. A zero rate or zero duration yields a
    /// factor of exactly `1.0` ([`ONE`]).
    fn growth_factor(&self, apr: i128, duration_seconds: u64) -> Result<i128, MathError>;
}

impl CompoundingStrategy for Compounding {
    fn growth_factor(&self, apr: i128, duration_seconds: u64) -> Result<i128, MathError> {
        if apr == 0 || duration_seconds == 0 {
            return Ok(ONE);
        }
        match self {
            Compounding::Daily => daily_growth_factor(apr, duration_seconds),
            Compounding::Continuous => continuous_growth_factor(apr, duration_seconds),
        }
    }
}

/// Growth factor for the daily-compounding model.
///
/// Computes `(1 + r/365)^d` for the whole-day count `d`, then multiplies by a
/// fractional factor `(1 + r/365 * frac)` for any partial trailing day, where
/// `frac` is the leftover seconds as a fraction of a day. This keeps partial
/// periods accurate without requiring a fractional exponent.
fn daily_growth_factor(apr: i128, duration_seconds: u64) -> Result<i128, MathError> {
    // Per-day rate: r / 365 in fixed-point. `apr` is already fixed-point and
    // 365 is a plain count, so this is ordinary integer division (using the
    // fixed-point `div` here would rescale by SCALE and inflate the rate).
    let daily_rate = apr / (DAYS_PER_YEAR as i128);
    let per_day_factor = ONE.checked_add(daily_rate).ok_or(MathError::Overflow)?;

    let whole_days = duration_seconds / SECONDS_PER_DAY;
    let mut factor = fp::pow_uint(per_day_factor, whole_days)?;

    // Handle the partial trailing day, if any, with simple (linear) interest
    // over the fraction of the day — this is the standard convention for the
    // sub-period remainder of a discrete compounding schedule.
    let remainder_seconds = duration_seconds % SECONDS_PER_DAY;
    if remainder_seconds > 0 {
        let frac = fp::div(remainder_seconds as i128, SECONDS_PER_DAY as i128)?;
        let partial_interest = fp::mul(daily_rate, frac)?;
        let partial_factor = ONE
            .checked_add(partial_interest)
            .ok_or(MathError::Overflow)?;
        factor = fp::mul(factor, partial_factor)?;
    }

    Ok(factor)
}

/// Growth factor for the continuous-compounding model: `e^(r * t)`.
///
/// `t` is the duration expressed as a fixed-point fraction of a year.
fn continuous_growth_factor(apr: i128, duration_seconds: u64) -> Result<i128, MathError> {
    // t = duration_seconds / SECONDS_PER_YEAR, in fixed-point.
    let t = fp::div(duration_seconds as i128, SECONDS_PER_YEAR as i128)?;
    let exponent = fp::mul(apr, t)?;
    fp::exp(exponent)
}

/// Computes yields for staked positions using a selectable compounding model.
///
/// The calculator is stateless — it is constructed with a [`Compounding`] mode
/// and then queried. This makes it cheap to hold in contract storage or create
/// on demand.
#[derive(Debug, Clone, Copy)]
pub struct YieldCalculator {
    compounding: Compounding,
}

impl YieldCalculator {
    /// Create a calculator using the given compounding model.
    pub fn new(compounding: Compounding) -> Self {
        Self { compounding }
    }

    /// The compounding model this calculator applies.
    pub fn compounding(&self) -> Compounding {
        self.compounding
    }

    /// Compute the yield (interest earned) on `principal` at annual rate `apr`
    /// over `duration_seconds`.
    ///
    /// Returns the earned amount in the same units as `principal` (i.e. token
    /// base units, *not* fixed-point). `apr` is fixed-point (e.g. `0.05 * SCALE`
    /// for 5%). The result excludes the principal — it is `P * (factor - 1)`.
    pub fn compute_yield(
        &self,
        principal: i128,
        apr: i128,
        duration_seconds: u64,
    ) -> Result<i128, MathError> {
        if principal <= 0 {
            return Ok(0);
        }
        let factor = self.compounding.growth_factor(apr, duration_seconds)?;
        // earned = principal * (factor - 1). factor and (factor-1) are
        // fixed-point; multiplying by the raw principal and rescaling gives the
        // earned amount in base units.
        let growth = factor.checked_sub(ONE).ok_or(MathError::Overflow)?;
        fp::mul(principal, growth)
    }

    /// Compute the final balance (principal + yield) on `principal` at annual
    /// rate `apr` over `duration_seconds`.
    pub fn compute_balance(
        &self,
        principal: i128,
        apr: i128,
        duration_seconds: u64,
    ) -> Result<i128, MathError> {
        if principal <= 0 {
            return Ok(principal);
        }
        let factor = self.compounding.growth_factor(apr, duration_seconds)?;
        fp::mul(principal, factor)
    }

    /// Compute yield across a sequence of `(apr, duration_seconds)` segments,
    /// compounding the balance through each in turn.
    ///
    /// This is how the engine accounts for **variable rates over time**: each
    /// segment represents a span during which the rate was constant, and the
    /// balance rolls forward from one segment into the next (time-weighted).
    /// Returns the total yield earned across all segments.
    pub fn compute_yield_segments(
        &self,
        principal: i128,
        segments: &[(i128, u64)],
    ) -> Result<i128, MathError> {
        if principal <= 0 {
            return Ok(0);
        }
        let mut balance = principal;
        for &(apr, duration) in segments {
            balance = self.compute_balance(balance, apr, duration)?;
        }
        balance.checked_sub(principal).ok_or(MathError::Overflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed_point::SCALE;

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

    #[test]
    fn zero_rate_yields_nothing() {
        let calc = YieldCalculator::new(Compounding::Daily);
        assert_eq!(
            calc.compute_yield(1_000_000, 0, SECONDS_PER_YEAR).unwrap(),
            0
        );
    }

    #[test]
    fn zero_duration_yields_nothing() {
        let calc = YieldCalculator::new(Compounding::Continuous);
        assert_eq!(calc.compute_yield(1_000_000, SCALE / 10, 0).unwrap(), 0);
    }

    #[test]
    fn daily_one_year_matches_formula() {
        // 5% APR, daily compounding, 1 year.
        // (1 + 0.05/365)^365 - 1 = 0.0512674965...
        // On a principal of 1e18 base units, yield ~= 5.12674965e16.
        let calc = YieldCalculator::new(Compounding::Daily);
        let principal = SCALE; // 1e18
        let apr = SCALE / 20; // 0.05
        let earned = calc
            .compute_yield(principal, apr, SECONDS_PER_YEAR)
            .unwrap();
        let expected = 51_267_496_505_408_400i128; // ~0.05126749650...
                                                   // within 1e-6 relative -> tol 1e11 on 1e18 scale
        approx(earned, expected, 100_000_000_000);
    }

    #[test]
    fn continuous_one_year_matches_formula() {
        // 5% APR, continuous, 1 year: e^0.05 - 1 = 0.0512710963...
        let calc = YieldCalculator::new(Compounding::Continuous);
        let principal = SCALE;
        let apr = SCALE / 20;
        let earned = calc
            .compute_yield(principal, apr, SECONDS_PER_YEAR)
            .unwrap();
        let expected = 51_271_096_376_024_040i128; // e^0.05 - 1
        approx(earned, expected, 100_000_000_000);
    }

    #[test]
    fn continuous_exceeds_daily() {
        // Continuous compounding must always yield at least as much as daily.
        let principal = 1_000_000_000i128;
        let apr = SCALE / 10; // 10%
        let daily = YieldCalculator::new(Compounding::Daily)
            .compute_yield(principal, apr, SECONDS_PER_YEAR)
            .unwrap();
        let cont = YieldCalculator::new(Compounding::Continuous)
            .compute_yield(principal, apr, SECONDS_PER_YEAR)
            .unwrap();
        assert!(
            cont >= daily,
            "continuous {} should be >= daily {}",
            cont,
            daily
        );
    }

    #[test]
    fn partial_period_half_day() {
        // Half a day at 36.5% APR daily: daily rate = 0.001, half day linear
        // interest = 0.0005. On principal 1e18 -> yield ~= 5e14.
        let calc = YieldCalculator::new(Compounding::Daily);
        let principal = SCALE;
        let apr = 365 * SCALE / 1000; // 0.365 -> daily rate 0.001
        let earned = calc
            .compute_yield(principal, apr, SECONDS_PER_DAY / 2)
            .unwrap();
        let expected = 500_000_000_000_000i128; // 0.0005 * 1e18
        approx(earned, expected, 1_000_000);
    }

    #[test]
    fn variable_rate_segments() {
        // Half a year at 4%, then half a year at 8%, continuous.
        // factor = e^(0.04*0.5) * e^(0.08*0.5) = e^0.02 * e^0.04 = e^0.06.
        // yield = e^0.06 - 1 = 0.0618365...
        let calc = YieldCalculator::new(Compounding::Continuous);
        let principal = SCALE;
        let half_year = SECONDS_PER_YEAR / 2;
        let segs = [(4 * SCALE / 100, half_year), (8 * SCALE / 100, half_year)];
        let earned = calc.compute_yield_segments(principal, &segs).unwrap();
        let expected = 61_836_546_545_359_750i128; // e^0.06 - 1
        approx(earned, expected, 1_000_000_000);
    }
}
