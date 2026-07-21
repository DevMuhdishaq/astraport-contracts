//! Projected future-earnings estimates.
//!
//! Given a position's principal, rate, and compounding model, this module
//! produces a [`YieldProjection`] describing expected earnings over a future
//! horizon. Projections reuse the exact same compounding math as realized
//! yield, so a 30-day forecast is consistent with what will actually accrue if
//! the rate holds — satisfying the "accurate to within 1% for 30-day forecast"
//! criterion by construction.

use crate::apy::APYCalculator;
use crate::compounding::{Compounding, YieldCalculator};
use crate::fixed_point::MathError;
use crate::records::{CompoundingMode, YieldProjection};

/// Builds [`YieldProjection`]s for staked positions.
pub struct YieldProjector;

impl YieldProjector {
    /// Project earnings for `principal` at `apr` under `mode` over
    /// `horizon_seconds`.
    ///
    /// Returns a fully-populated [`YieldProjection`], including the effective
    /// APY implied by the rate and model.
    pub fn project(
        principal: i128,
        apr: i128,
        mode: CompoundingMode,
        horizon_seconds: u64,
    ) -> Result<YieldProjection, MathError> {
        let strategy: Compounding = mode.to_strategy();
        let calc = YieldCalculator::new(strategy);

        let projected_yield = calc.compute_yield(principal, apr, horizon_seconds)?;
        let projected_balance = principal
            .checked_add(projected_yield)
            .ok_or(MathError::Overflow)?;
        let effective_apy = APYCalculator::apr_to_apy(apr, strategy)?;

        Ok(YieldProjection {
            principal,
            apr,
            mode,
            horizon_seconds,
            projected_yield,
            projected_balance,
            effective_apy,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed_point::{SCALE, SECONDS_PER_DAY};

    #[test]
    fn thirty_day_projection_is_reasonable() {
        // 10% APR, continuous, 30 days on 1e18 principal.
        // t = 30/365; yield = 1e18 * (e^(0.1*30/365) - 1).
        // 0.1*30/365 = 0.0082192; e^that - 1 = 0.0082530...
        let horizon = 30 * SECONDS_PER_DAY;
        let p = SCALE;
        let apr = SCALE / 10;
        let proj = YieldProjector::project(p, apr, CompoundingMode::Continuous, horizon).unwrap();
        // expected ~ 8.2530e15
        let expected = 8_253_048_640_000_000i128;
        let diff = (proj.projected_yield - expected).abs();
        // within 1% of expected
        assert!(diff <= expected / 100, "diff {} exceeds 1% of {}", diff, expected);
        assert_eq!(proj.projected_balance, p + proj.projected_yield);
    }
}
