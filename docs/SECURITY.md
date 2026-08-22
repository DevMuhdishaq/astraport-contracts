# AstraPort Smart Contracts — Security Best Practices

> Guidelines for secure development, deployment, and operation of AstraPort contracts.

---

## Table of Contents

1. [General Principles](#1-general-principles)
2. [Authorization & Access Control](#2-authorization--access-control)
3. [Input Validation](#3-input-validation)
4. [Yield Engine Security](#4-yield-engine-security)
5. [Emergency Unstaking Security](#5-emergency-unstaking-security)
6. [Audit Trail Integrity](#6-audit-trail-integrity)
7. [Rebalancing Security](#7-rebalancing-security)
8. [Key Management](#8-key-management)
9. [Deployment Security](#9-deployment-security)
10. [Incident Response](#10-incident-response)

---

## 1. General Principles

### 1.1 Defense in Depth

- Every public function validates its inputs independently.
- Authorization is enforced at the contract level, not just the client level.
- The audit contract provides an immutable, tamper-evident record of all state changes.

### 1.2 Least Privilege

- Contracts use `#![no_std]` to minimize attack surface.
- Admin functions require both `require_auth()` and admin address verification.
- Emergency functions have cooldown periods and penalty mechanisms.

### 1.3 Deterministic Execution

- All math uses fixed-point arithmetic (no floating point).
- Every validator computes identical results.
- SHA-256 chain hashing ensures tamper detection.

---

## 2. Authorization & Access Control

### 2.1 Always Require Authorization

Every state-changing function must call `require_auth()` on the appropriate address:

```rust
// CORRECT: Require authorization
pub fn stake(env: Env, staker: Address, asset: Symbol, amount: i128) -> Result<Symbol, Error> {
    staker.require_auth();
    // ... state changes
}

// WRONG: No authorization check
pub fn stake(env: Env, staker: Address, asset: Symbol, amount: i128) -> Result<Symbol, Error> {
    // Missing: staker.require_auth();
    // ... state changes — ANYONE could call this with ANY staker address
}
```

### 2.2 Admin Verification

Admin-only functions must verify both authorization AND identity:

```rust
fn assert_admin(env: &Env, admin: &Address) {
    let stored_admin: Address = env.storage().persistent().get(&YieldDataKey::Admin)
        .expect("contract not initialized");
    assert!(stored_admin == *admin, "caller is not admin");
}

pub fn set_alert_threshold(env: Env, admin: Address, threshold: i128) -> Symbol {
    admin.require_auth();                    // 1. Verify authorization
    Self::assert_admin(&env, &admin);        // 2. Verify identity
    // ... state change
}
```

### 2.3 Portfolio Ownership

The rebalancing contract enforces portfolio-level ownership:

```rust
fn require_owner_auth(env: &Env, owner: &Address, portfolio_id: &Symbol) -> Result<(), Error> {
    owner.require_auth();
    let key = DataKey::Owner(portfolio_id.clone());
    if let Some(stored_owner) = env.storage().persistent().get(&key) {
        if &stored_owner != owner {
            return Err(RebalancingError::Unauthorized);
        }
    } else {
        // First caller becomes the owner
        env.storage().persistent().set(&key, owner);
    }
    Ok(())
}
```

### 2.4 Cross-Contract Trust

The audit contract trusts calling contracts to have authorized the `actor`:

```rust
// The audit contract does NOT call actor.require_auth() itself.
// The calling contract (staking, rebalancing) is responsible.
pub fn log_event(env: Env, actor: Address, ...) -> u64 {
    // Trusts that `actor` was already authorized by the caller
}
```

**Risk:** A compromised or buggy calling contract could log false audit events.

**Mitigation:** The chain-hash integrity check detects any tampering with the audit log.

---

## 3. Input Validation

### 3.1 Positive Amounts

Always reject zero and negative amounts:

```rust
pub fn stake(env: Env, staker: Address, asset: Symbol, amount: i128) -> Result<Symbol, Error> {
    if amount <= 0 {
        return Err(Error::InvalidStakeAmount);
    }
    // ...
}
```

### 3.2 Balance Bounds

Never allow operations that exceed available balance:

```rust
pub fn unstake(env: Env, staker: Address, asset: Symbol, amount: i128) -> Result<Symbol, Error> {
    let current_balance: i128 = env.storage().persistent().get(&key).unwrap_or_default();
    if amount > current_balance {
        return Err(Error::InsufficientBalance);
    }
    // ...
}
```

### 3.3 Allocation Validation

Rebalancing allocations must sum to exactly 10,000 basis points:

```rust
pub fn set_target_allocation(env: Env, ..., allocation: TargetAllocation) -> Result<Symbol, Error> {
    let mut total: u32 = 0;
    for (_asset, weight) in allocation.allocations.iter() {
        total += weight;
    }
    if total != 10_000 {
        return Err(RebalancingError::InvalidAllocation);
    }
    // ...
}
```

### 3.4 Basis Point Clamping

Emergency penalty rates must be in [0, 10,000]:

```rust
let bps = /* computed penalty */;
Ok(bps.max(0).min(MAX_BPS)) // Clamp to valid range
```

---

## 4. Yield Engine Security

### 4.1 Fixed-Point Precision

- All rates use 18-decimal fixed-point (`SCALE = 1e18`).
- Intermediate calculations use 256-bit arithmetic to prevent overflow.
- Results are accurate to within 0.01% for APY calculations.

### 4.2 Overflow Protection

The `mul_div` function uses 256-bit intermediate products:

```rust
pub fn mul_div(a: i128, b: i128, denom: i128) -> Result<i128, MathError> {
    let prod = mul_u128_to_u256(a, b); // 256-bit intermediate
    let (q, _r) = div_u256_by_u128(prod, denom)?;
    // ...
}
```

### 4.3 Time-Weighted Rate Changes

Rate changes checkpoint accrued yield at the old rate before applying the new rate:

```rust
pub fn set_rate(&self, staker: &Address, asset: &Symbol, new_apr: i128) -> Result<YieldRecord, MathError> {
    let mut updated = self.accrue_to(&record, now)?; // Checkpoint at old rate
    updated.apr = new_apr;                           // Apply new rate
    self.store_record(&updated);
    Ok(updated)
}
```

**Why:** Without checkpointing, a rate change would incorrectly apply the new rate to the entire history.

### 4.4 No Yield on Zero Principal

Yield positions with zero principal should not accrue yield:

```rust
pub fn compute_yield(&self, principal: i128, apr: i128, duration_seconds: u64) -> Result<i128, MathError> {
    if principal <= 0 {
        return Ok(0);
    }
    // ...
}
```

---

## 5. Emergency Unstaking Security

### 5.1 Cooldown Period

The cooldown prevents rapid emergency unstakes:

```rust
let cooldown_end: u64 = env.storage().persistent()
    .get(&EmergencyDataKey::CooldownEnd(staker.clone()))
    .unwrap_or(0);
assert!(now >= cooldown_end, "CooldownActive");
```

### 5.2 Penalty Decay

Penalties decay over time, incentivizing users to wait:

- **Linear:** `penalty(t) = start + (end - start) * elapsed / total`
- **Exponential:** `penalty(t) = start * (end / start)^(elapsed / total)`

### 5.3 Treasury Distribution

Penalties are distributed to a designated treasury address via events:

```rust
env.events().publish(
    (symbol_short!("PENALTY"), staker.clone()),
    PenaltyDistributionEvent { staker, treasury, penalty_amount, timestamp },
);
```

**Note:** Actual token transfers are handled off-chain or by a future token integration.

### 5.4 Lock Position Management

Only the admin can set lock positions:

```rust
pub fn set_lock_position(env: Env, admin: Address, staker: Address, ...) -> Symbol {
    admin.require_auth();
    Self::assert_admin(&env, &admin);
    // ...
}
```

**Why:** Stakers should not be able to extend their own lock to reduce their penalty.

---

## 6. Audit Trail Integrity

### 6.1 SHA-256 Chain Hash

Each audit entry includes a hash binding it to all prior entries:

```
hash(n) = SHA-256(hash(n-1) || serialize(entry_n))
```

Any tampering with entry `k` breaks the chain for all subsequent entries.

### 6.2 Integrity Verification

Three levels of integrity checking:

1. **Head check:** Compare stored head to expected hash.
2. **Full recompute:** Recompute the entire chain from scratch.
3. **Chain link check:** Verify each entry links to the previous.

```rust
// Quick check
let valid = audit_client.verify_integrity(&expected_head);

// Comprehensive check
let valid = audit_client.full_recompute_integrity();
```

### 6.3 Retention Policy

Retention policies prune old entries while maintaining chain integrity:

```rust
let policy = RetentionPolicy {
    max_entries: 10_000,
    max_age_seconds: 86400 * 365,
};
audit_client.set_retention_policy(&admin, &policy)?;
audit_client.prune_old(&admin)?; // Recomputes chain head after pruning
```

---

## 7. Rebalancing Security

### 7.1 Allocation Sums

Target allocations and current holdings must sum to exactly 10,000 bps:

```rust
if total != 10_000 {
    return Err(RebalancingError::InvalidAllocation);
}
```

### 7.2 Drift Thresholds

The drift threshold prevents unnecessary rebalancing:

```rust
fn add_adjustment_if_needed(adjustments: &mut Vec<RebalanceAdjustment>, ..., threshold: u32) {
    let drift = current_weight as i32 - target_weight as i32;
    if drift.unsigned_abs() > threshold {
        // Only flag for rebalancing if drift exceeds threshold
    }
}
```

### 7.3 Schedule Validation

Rebalancing intervals are validated before storage:

```rust
pub fn validate(interval: &RebalanceInterval) -> bool {
    match interval {
        RebalanceInterval::Hourly
        | RebalanceInterval::Daily
        | RebalanceInterval::Weekly
        | RebalanceInterval::Monthly => true,
    }
}
```

### 7.4 Owner-Only Execution

Manual rebalancing requires portfolio ownership:

```rust
pub fn rebalance(env: Env, owner: Address, portfolio_id: Symbol) -> Result<RebalanceResult, Error> {
    Self::require_owner_auth(&env, &owner, &portfolio_id)?;
    // ...
}
```

---

## 8. Key Management

### 8.1 Admin Keys

- Store admin keys in a hardware wallet or secure enclave.
- Never commit private keys to version control.
- Use separate keys for testnet and mainnet.

### 8.2 Deployer Keys

- The deployer key has significant power during deployment.
- Rotate deployer keys after deployment is complete.
- Consider using a multisig for mainnet deployment.

### 8.3 Treasury Keys

- Treasury keys receive penalty collections.
- Use a multisig or cold storage for treasury funds.
- Monitor treasury balance regularly.

---

## 9. Deployment Security

### 9.1 Testnet First

Always deploy and test on testnet before mainnet:

```bash
# Deploy to testnet
soroban contract deploy --wasm <WASM> --source deployer --network testnet

# Run integration tests against testnet
cargo test --features testnet
```

### 9.2 Immutable Contracts

AstraPort contracts do not include upgrade mechanisms. This is a security feature:

- No proxy patterns that could be hijacked.
- No admin functions that could be exploited for upgrades.
- Deployment is a one-time event.

### 9.3 Verification

After deployment, verify:

1. All contracts are initialized.
2. Audit sinks are configured.
3. Emergency systems work as expected.
4. Integrity checks pass.

### 9.4 Mainnet Checklist

- [ ] All testnet tests pass
- [ ] Security audit completed
- [ ] Admin keys are secure
- [ ] Treasury is configured
- [ ] Retention policies are set
- [ ] Rollback plan is documented
- [ ] Team is notified of deployment

---

## 10. Incident Response

### 10.1 If a Bug Is Found

1. **Assess severity:** Is user funds at risk?
2. **Disable affected functions:** If possible, disable the vulnerable function.
3. **Notify users:** Communicate the issue and recommended actions.
4. **Deploy fix:** Deploy a patched contract version if needed.

### 10.2 If Unauthorized Access Is Detected

1. **Disable emergency unstaking** to prevent fund drainage.
2. **Review audit logs** for evidence of tampering.
3. **Check integrity** with `full_recompute_integrity()`.
4. **Contact the team** immediately.

### 10.3 If Integrity Check Fails

1. **Stop all operations** on the affected contract.
2. **Export audit logs** before they're lost.
3. **Investigate** the tampering.
4. **Deploy a new contract** with corrected data if possible.

---

## Reporting Security Issues

If you discover a security vulnerability:

1. **Do NOT** open a public GitHub issue.
2. **Do NOT** discuss the vulnerability publicly.
3. **Email** the security team at: [security contact to be added]
4. **Include:**
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if any)

We will respond within 48 hours and work with you to address the issue.
