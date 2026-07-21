//! Fixed-point arithmetic primitives for deterministic yield math.
//!
//! Soroban contracts must be fully deterministic across validators, so this
//! module deliberately avoids floating point entirely. All rates and factors
//! are represented as integers scaled by [`SCALE`] (1e18), giving 18 decimal
//! places of precision. The helpers below implement the handful of operations
//! the yield engine needs — multiply, divide, integer power, and the
//! exponential function `e^x` (via a Taylor series) — all in pure `i128`
//! integer math.
//!
//! # Representation
//!
//! A real number `x` is stored as `round(x * SCALE)`. For example:
//! - `1.0`  -> `1_000_000_000_000_000_000`
//! - `0.05` ->    `50_000_000_000_000_000`
//!
//! Intermediate products are widened to `i256` (emulated via [`mul_div`]) to
//! avoid overflow before rescaling back down.

/// The scaling factor used for all fixed-point values: 1e18.
///
/// This gives 18 fractional decimal digits, matching the convention used by
/// most token systems and comfortably exceeding the 0.01% APY accuracy the
/// yield engine targets.
pub const SCALE: i128 = 1_000_000_000_000_000_000;

/// One whole unit (1.0) in fixed-point representation. Alias for [`SCALE`].
pub const ONE: i128 = SCALE;

/// Number of seconds in a standard (365-day) year, used for APR/APY math.
pub const SECONDS_PER_YEAR: u64 = 365 * 24 * 60 * 60;

/// Number of seconds in a day.
pub const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

/// Number of compounding periods per year for the daily model.
pub const DAYS_PER_YEAR: u64 = 365;

/// Errors that can arise from fixed-point operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathError {
    /// A multiplication or addition overflowed the `i128` range.
    Overflow,
    /// A division by zero was attempted.
    DivideByZero,
    /// A negative input was supplied where only non-negative values are valid.
    NegativeInput,
}

/// Multiply two fixed-point numbers, returning a fixed-point result.
///
/// Computes `(a * b) / SCALE` with a widened intermediate so the multiplication
/// does not overflow for realistic inputs.
pub fn mul(a: i128, b: i128) -> Result<i128, MathError> {
    mul_div(a, b, SCALE)
}

/// Divide two fixed-point numbers, returning a fixed-point result.
///
/// Computes `(a * SCALE) / b`.
pub fn div(a: i128, b: i128) -> Result<i128, MathError> {
    if b == 0 {
        return Err(MathError::DivideByZero);
    }
    mul_div(a, SCALE, b)
}

/// Compute `a * b / denom` using a 256-bit intermediate to avoid overflow.
///
/// This is the workhorse behind [`mul`] and [`div`]. The multiplication is
/// performed in emulated 256-bit precision so that `a * b` never overflows
/// before the final division brings the magnitude back down.
pub fn mul_div(a: i128, b: i128, denom: i128) -> Result<i128, MathError> {
    if denom == 0 {
        return Err(MathError::DivideByZero);
    }

    // Track the sign separately and operate on magnitudes.
    let neg = (a < 0) ^ (b < 0) ^ (denom < 0);
    let a = a.unsigned_abs();
    let b = b.unsigned_abs();
    let d = denom.unsigned_abs();

    let prod = mul_u128_to_u256(a, b);
    let (q, _r) = div_u256_by_u128(prod, d)?;

    // The quotient must fit back into i128.
    if q > i128::MAX as u128 {
        return Err(MathError::Overflow);
    }
    let q = q as i128;
    Ok(if neg { -q } else { q })
}

/// A minimal unsigned 256-bit integer represented as two 128-bit limbs.
///
/// `hi` holds the most-significant 128 bits, `lo` the least-significant. Only
/// the operations required by [`mul_div`] are implemented.
#[derive(Clone, Copy)]
struct U256 {
    hi: u128,
    lo: u128,
}

/// Multiply two `u128` values into a full 256-bit product with no loss.
fn mul_u128_to_u256(a: u128, b: u128) -> U256 {
    // Split each operand into 64-bit halves and use schoolbook multiplication.
    let a_lo = a & 0xFFFF_FFFF_FFFF_FFFF;
    let a_hi = a >> 64;
    let b_lo = b & 0xFFFF_FFFF_FFFF_FFFF;
    let b_hi = b >> 64;

    let ll = a_lo * b_lo;
    let lh = a_lo * b_hi;
    let hl = a_hi * b_lo;
    let hh = a_hi * b_hi;

    // Accumulate the cross terms, carrying into the high limb.
    let mut lo = ll;
    let mut hi = hh;

    // Add lh << 64.
    let (mid1, carry1) = lo.overflowing_add(lh << 64);
    lo = mid1;
    hi += (lh >> 64) + carry1 as u128;

    // Add hl << 64.
    let (mid2, carry2) = lo.overflowing_add(hl << 64);
    lo = mid2;
    hi += (hl >> 64) + carry2 as u128;

    U256 { hi, lo }
}

/// Divide a 256-bit numerator by a 128-bit divisor.
///
/// Returns `(quotient, remainder)`. Uses binary long division, which is
/// sufficient for the modest throughput of a smart contract. Returns
/// [`MathError::Overflow`] if the quotient would not fit in `u128`.
fn div_u256_by_u128(num: U256, den: u128) -> Result<(u128, u128), MathError> {
    if den == 0 {
        return Err(MathError::DivideByZero);
    }
    // Fast path: numerator fits in a single limb.
    if num.hi == 0 {
        return Ok((num.lo / den, num.lo % den));
    }

    // Binary long division over the 256-bit value, MSB first.
    let mut quotient_hi: u128 = 0;
    let mut quotient_lo: u128 = 0;
    let mut rem: u128 = 0;

    for i in (0..256).rev() {
        // Shift remainder left by one and bring in the next numerator bit.
        let bit = if i >= 128 {
            (num.hi >> (i - 128)) & 1
        } else {
            (num.lo >> i) & 1
        };

        // rem = (rem << 1) | bit. rem is guaranteed < den < 2^128, and den>=1,
        // so rem < 2^128 - 1 before the shift only when den <= 2^127; to be
        // safe against the shift overflowing we rely on rem < den <= u128::MAX,
        // meaning the top bit of rem is only set when den itself is that large,
        // in which case rem << 1 is handled by comparing before subtracting.
        let overflow_top = rem >> 127;
        rem = (rem << 1) | bit;

        if overflow_top == 1 || rem >= den {
            rem = rem.wrapping_sub(den);
            // Set bit i in the quotient.
            if i >= 128 {
                quotient_hi |= 1u128 << (i - 128);
            } else {
                quotient_lo |= 1u128 << i;
            }
        }
    }

    if quotient_hi != 0 {
        return Err(MathError::Overflow);
    }
    Ok((quotient_lo, rem))
}

/// Raise a fixed-point base to an integer power using exponentiation by
/// squaring.
///
/// `base` is in fixed-point; `exp` is an ordinary non-negative integer. Returns
/// `base^exp` in fixed-point. `base^0` is [`ONE`].
pub fn pow_uint(base: i128, exp: u64) -> Result<i128, MathError> {
    let mut result = ONE;
    let mut acc = base;
    let mut e = exp;

    while e > 0 {
        if e & 1 == 1 {
            result = mul(result, acc)?;
        }
        e >>= 1;
        if e > 0 {
            acc = mul(acc, acc)?;
        }
    }
    Ok(result)
}

/// Compute `e^x` for a fixed-point exponent `x` via a Taylor series.
///
/// The series `1 + x + x^2/2! + x^3/3! + ...` converges quickly for the small
/// exponents used in continuous-compounding yield math (typically `|x| < 1`).
/// For accuracy and to keep convergence fast, the exponent is range-reduced:
/// `e^x = (e^(x/2^k))^(2^k)`. We pick `k` so the reduced exponent is below
/// ~0.5 in magnitude, then square the result back up.
///
/// Only non-negative `x` is supported, which is all the yield engine requires
/// (rates and durations are non-negative).
pub fn exp(x: i128) -> Result<i128, MathError> {
    if x < 0 {
        return Err(MathError::NegativeInput);
    }
    if x == 0 {
        return Ok(ONE);
    }

    // Range-reduce: halve the exponent until |x| <= 0.5 (in fixed-point,
    // SCALE/2), remembering how many halvings we performed.
    let half = SCALE / 2;
    let mut k: u32 = 0;
    let mut reduced = x;
    while reduced > half {
        reduced /= 2;
        k += 1;
    }

    // Taylor series on the reduced exponent.
    // term_0 = 1, term_{n} = term_{n-1} * x / n.
    let mut term = ONE;
    let mut sum = ONE;
    // 20 terms is far more than enough for |x| <= 0.5 to reach 1e-18 precision.
    for n in 1..=20u64 {
        term = mul(term, reduced)?;
        term /= n as i128;
        sum = sum
            .checked_add(term)
            .ok_or(MathError::Overflow)?;
        if term == 0 {
            break;
        }
    }

    // Square the result k times to undo the range reduction.
    let mut result = sum;
    for _ in 0..k {
        result = mul(result, result)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert two fixed-point values are within `tol` (fixed-point) of each other.
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
    fn mul_identity() {
        assert_eq!(mul(ONE, ONE).unwrap(), ONE);
        assert_eq!(mul(2 * ONE, 3 * ONE).unwrap(), 6 * ONE);
    }

    #[test]
    fn mul_fractional() {
        // 0.5 * 0.5 = 0.25
        let half = SCALE / 2;
        approx(mul(half, half).unwrap(), SCALE / 4, 1);
    }

    #[test]
    fn div_basic() {
        // 1 / 4 = 0.25
        approx(div(ONE, 4 * ONE).unwrap(), SCALE / 4, 1);
        // 3 / 2 = 1.5
        approx(div(3 * ONE, 2 * ONE).unwrap(), 3 * SCALE / 2, 1);
    }

    #[test]
    fn div_by_zero_errors() {
        assert_eq!(div(ONE, 0), Err(MathError::DivideByZero));
    }

    #[test]
    fn mul_div_no_overflow_large() {
        // Large operands whose product exceeds i128 must still work via the
        // 256-bit intermediate. (1e30) * (1e30) / (1e30) = 1e30.
        let big = 1_000_000_000_000_000_000_000_000_000_000i128; // 1e30
        let r = mul_div(big, big, big).unwrap();
        assert_eq!(r, big);
    }

    #[test]
    fn pow_uint_basic() {
        // 2^10 = 1024
        approx(pow_uint(2 * ONE, 10).unwrap(), 1024 * ONE, 1000);
        // anything^0 = 1
        assert_eq!(pow_uint(5 * ONE, 0).unwrap(), ONE);
        // (1.05)^2 = 1.1025
        let base = ONE + SCALE / 20; // 1.05
        approx(pow_uint(base, 2).unwrap(), 11_025 * SCALE / 10_000, SCALE / 1_000_000);
    }

    #[test]
    fn exp_zero_is_one() {
        assert_eq!(exp(0).unwrap(), ONE);
    }

    #[test]
    fn exp_one_is_e() {
        // e^1 ~= 2.718281828459045235
        let e = 2_718_281_828_459_045_235i128;
        // within 1e-12 (tol = 1e6 in fixed-point 1e18 scale)
        approx(exp(ONE).unwrap(), e, 1_000_000);
    }

    #[test]
    fn exp_small_value() {
        // e^0.05 ~= 1.051271096376024040
        let x = SCALE / 20; // 0.05
        let expected = 1_051_271_096_376_024_040i128;
        approx(exp(x).unwrap(), expected, 1_000_000);
    }
}
