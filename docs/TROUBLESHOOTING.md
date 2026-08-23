# AstraPort Smart Contracts — Troubleshooting Guide

> Solutions for common issues encountered when building, testing, and deploying AstraPort contracts.

---

## Table of Contents

1. [Build Issues](#1-build-issues)
2. [Test Failures](#2-test-failures)
3. [Deployment Issues](#3-deployment-issues)
4. [Runtime Errors](#4-runtime-errors)
5. [Yield Engine Issues](#5-yield-engine-issues)
6. [Rebalancing Issues](#6-rebalancing-issues)
7. [Audit Contract Issues](#7-audit-contract-issues)
8. [Performance Issues](#8-performance-issues)

---

## 1. Build Issues

### Missing WASM target

**Symptom:** `error: target 'wasm32-unknown-unknown' not found`

**Solution:**
```bash
rustup target add wasm32-unknown-unknown
```

### Soroban SDK version mismatch

**Symptom:** Compilation errors referencing missing types or traits.

**Solution:** Ensure your `Cargo.toml` uses the workspace dependency:
```toml
soroban-sdk = { workspace = true }
```
The workspace pins `soroban-sdk = "=21.5.0"`. Do not override this.

### Clippy warnings about too many arguments

**Symptom:** `warning: this function has too many arguments`

**Solution:** This is expected for Soroban contract entrypoints. The crate-level `#![allow(clippy::too_many_arguments)]` suppresses it. If adding new entrypoints, the allow is already in `lib.rs`.

### Build fails with `transmute` errors

**Symptom:** Errors related to `transmute` in dependency crates.

**Solution:** Ensure `ethnum >= 1.5.2` in `Cargo.lock`. Run:
```bash
cargo update -p ethnum
```

---

## 2. Test Failures

### `mock_all_auths` not working

**Symptom:** Tests fail with authorization errors.

**Solution:** Ensure `env.mock_all_auths()` is called before any contract interactions in the test:
```rust
let env = Env::default();
env.mock_all_auths();
```

### Stale test cache

**Symptom:** Tests pass individually but fail when run together, or vice versa.

**Solution:** Clear the build cache:
```bash
cargo clean
cargo test
```

### Panics in tests

**Symptom:** Test panics with "already initialized" or "not initialized".

**Solution:** Each test gets a fresh `Env` by default. If you're seeing this:
- Ensure you're not reusing contract state across tests.
- Each `Env::default()` creates an isolated environment.

### Yield calculation precision issues

**Symptom:** Yield values don't match expected values.

**Solution:** The yield engine uses 18-decimal fixed-point math. Ensure:
- APR values are scaled by `SCALE` (1e18).
- Use the `approx` helper in tests with appropriate tolerance.
- Continuous compounding always yields ≥ daily compounding.

---

## 3. Deployment Issues

### Contract already initialized

**Symptom:** `panic: already initialized` on deployment.

**Solution:** The `initialize` function can only be called once. If you need to re-initialize:
- Deploy a new contract instance.
- Or use the existing initialized contract.

### Insufficient account balance

**Symptom:** `insufficient funds` error during deployment.

**Solution:** Fund your Stellar account with test XLM:
```bash
# For testnet
curl "https://friendbot.stellar.org/?addr=<YOUR_ADDRESS>"
```

### WASM file not found

**Symptom:** `error: WASM file not found at target/wasm32-unknown-unknown/release/...`

**Solution:** Build the WASM first:
```bash
soroban contract build --package <PACKAGE_NAME>
```

---

## 4. Runtime Errors

### `InvalidStakeAmount`

**Cause:** Attempting to stake with `amount <= 0` or addition overflow.

**Solution:** Always pass a positive amount:
```rust
assert!(amount > 0, "Stake amount must be positive");
```

### `InsufficientBalance`

**Cause:** Attempting to unstake more than the current balance.

**Solution:** Check balance before unstaking:
```rust
let balance = client.get_balance(&staker, &asset);
assert!(amount <= balance, "Cannot unstake more than staked");
```

### `Unauthorized`

**Cause:** Caller is not the portfolio owner (rebalancing) or not the staker (staking).

**Solution:** Ensure the correct address is calling:
```rust
// In rebalancing, the first caller becomes the owner
client.set_target_allocation(&owner, &portfolio, &allocation)?;
// Only `owner` can modify this portfolio going forward
```

### `EmergencyUnstakeDisabled`

**Cause:** Emergency unstaking has not been enabled by the admin.

**Solution:** Admin must configure emergency unstaking:
```rust
client.configure_emergency_unstake(
    &admin, &penalty_start, &penalty_end, &decay,
    &cooldown, &treasury, &true, // enabled = true
)?;
```

### `CooldownActive`

**Cause:** Staker is attempting another emergency unstake before the cooldown period expires.

**Solution:** Wait for the cooldown to expire:
```rust
let cooldown_end = client.get_cooldown_end(&staker);
assert!(env.ledger().timestamp() >= cooldown_end, "Cooldown still active");
```

---

## 5. Yield Engine Issues

### Yield position not found

**Cause:** `accrue_yield` or `current_yield` called for a non-existent position.

**Solution:** Open a yield position first:
```rust
client.open_yield_position(&staker, &asset, &principal, &apr, &mode)?;
```

### Zero yield despite time passing

**Cause:** APR is 0 or position was just opened.

**Solution:** Verify the APR is set correctly (should be > 0 in fixed-point):
```rust
let apr = 50_000_000_000_000_000; // 5% in 1e18 scale
client.open_yield_position(&staker, &asset, &principal, &apr, &mode)?;
```

### Yield not updating

**Cause:** `current_yield` returns checkpointed yield only.

**Solution:** The `current_yield` function already includes pending (uncheckpointed) yield. If you're seeing stale values, ensure you're reading from the correct contract.

### APY/APR conversion seems wrong

**Cause:** Fixed-point precision expectations.

**Solution:** APR/APY conversion is accurate to within 0.01%. Use the conversion functions:
```rust
let apy = client.apr_to_apy(&apr, &CompoundingMode::Daily);
let recovered_apr = client.apy_to_apr(&apy, &CompoundingMode::Daily);
// recovered_apr ≈ apr (within 0.0001%)
```

---

## 6. Rebalancing Issues

### `InvalidAllocation`

**Cause:** Target allocation weights don't sum to exactly 10,000 basis points.

**Solution:** Ensure all weights sum to 10,000:
```rust
let total: u32 = allocations.iter().map(|(_, w)| w).sum();
assert_eq!(total, 10_000, "Allocation must sum to 10,000 bps");
```

### `TargetAllocationNotFound`

**Cause:** `rebalance` called before setting a target allocation.

**Solution:** Set target allocation before rebalancing:
```rust
client.set_target_allocation(&owner, &portfolio, &target)?;
client.set_current_holdings(&owner, &portfolio, &holdings)?;
client.rebalance(&owner, &portfolio)?;
```

### Schedule not executing

**Cause:** The scheduled time hasn't elapsed yet.

**Solution:** Check the schedule status:
```rust
let schedule = client.get_schedule(&portfolio).unwrap();
let now = env.ledger().timestamp();
assert!(now >= schedule.next_execution, "Schedule not yet due");
```

### Rebalance shows no adjustments

**Cause:** Current holdings are within the drift threshold of the target.

**Solution:** Either:
- Tighten the drift threshold: `set_drift_threshold_bps(&owner, &portfolio, &50)?`
- Or verify holdings actually differ from target

---

## 7. Audit Contract Issues

### `NotInitialized`

**Cause:** Audit contract `initialize` was not called.

**Solution:** Initialize before use:
```rust
audit_client.initialize(&admin_address)?;
```

### Chain integrity verification fails

**Cause:** Entries have been tampered with or the chain is empty.

**Solution:**
```rust
// Check if the chain is empty
let head = audit_client.integrity_head();
// If head matches CHAIN_ORIGIN (all zeros), the chain is empty

// Full recompute for detailed check
let is_valid = audit_client.full_recompute_integrity();
```

### `NoRetentionPolicy`

**Cause:** Attempting to prune without setting a retention policy.

**Solution:** Set a retention policy first:
```rust
let policy = RetentionPolicy {
    max_entries: 10_000,
    max_age_seconds: 86400 * 365,
};
audit_client.set_retention_policy(&admin, &policy)?;
audit_client.prune_old(&admin)?;
```

---

## 8. Performance Issues

### Slow queries

**Cause:** Large number of audit entries without filters.

**Solution:** Always use `LogQuery` with filters:
```rust
let query = LogQuery::new(100)
    .event_type(AuditEventType::Stake)
    .from_ts(start_time)
    .to_ts(end_time);
```

### High gas costs

**Cause:** Complex operations on large datasets.

**Solution:**
- Limit `Vec` sizes in queries.
- Use `limit` in `LogQuery` to cap results.
- Avoid `full_recompute_integrity` in hot paths.
- Cache frequently-read values off-chain.

### Storage quota exceeded

**Cause:** Too many on-chain storage entries.

**Solution:** Configure retention policies on the audit contract to prune old entries periodically.

---

## Getting Help

If you encounter an issue not covered here:

1. Check the existing [test files](../contracts/staking/src/tests.rs) for examples.
2. Review the [API Reference](API_REFERENCE.md) for function signatures.
3. Open a GitHub issue with:
   - The exact error message
   - Steps to reproduce
   - Your Rust/Soroban versions (`rustc --version`, `soroban --version`)
