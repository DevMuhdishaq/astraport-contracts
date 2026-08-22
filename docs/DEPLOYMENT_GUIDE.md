# AstraPort Smart Contracts — Deployment Guide

> Step-by-step instructions for deploying AstraPort contracts to Stellar testnet and mainnet.

---

## Table of Contents

1. [Prerequisites](#1-prerequisites)
2. [Build WASM Binaries](#2-build-wasm-binaries)
3. [Testnet Deployment](#3-testnet-deployment)
4. [Mainnet Deployment](#4-mainnet-deployment)
5. [Post-Deployment Configuration](#5-post-deployment-configuration)
6. [Deployment Order](#6-deployment-order)
7. [Verification](#7-verification)
8. [Rollback Plan](#8-rollback-plan)

---

## 1. Prerequisites

### Required Tools

```bash
# Rust 1.75.0+
rustc --version

# Soroban CLI v21.5.0+
soroban --version

# Stellar CLI (for network interaction)
stellar --version
```

### Required Accounts

| Purpose | Network | Requirements |
|---------|---------|--------------|
| Deployer | testnet/mainnet | Funded with XLM for contract deployment |
| Admin | testnet/mainnet | Address that will manage the contracts |
| Treasury | testnet/mainnet | Address for penalty collection (staking) |

### Fund Testnet Account

```bash
# Using Friendbot for testnet
curl "https://friendbot.stellar.org/?addr=<DEPLOYER_ADDRESS>"
```

---

## 2. Build WASM Binaries

### Debug Build (for testing)

```bash
cargo build
```

### Release Build (optimized for deployment)

```bash
soroban contract build --package astraport-rebalancing
soroban contract build --package astraport-events
soroban contract build --package astraport-staking
soroban contract build --package astraport-audit
```

### Verify WASM Files

```bash
ls -la target/wasm32-unknown-unknown/release/*.wasm
```

Expected files:
- `astraport_rebalancing.wasm`
- `astraport_events.wasm`
- `astraport_staking.wasm`
- `astraport_audit.wasm`

---

## 3. Testnet Deployment

### Step 3.1: Configure Network

```bash
stellar network use testnet
```

### Step 3.2: Deploy Contracts (in order)

Deploy in the following order due to dependencies:

**3.2.1. Audit Contract** (deployed first — other contracts reference it)

```bash
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/astraport_audit.wasm \
  --source deployer \
  --network testnet
```

Save the returned contract address as `AUDIT_CONTRACT_ID`.

**3.2.2. Events Contract**

```bash
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/astraport_events.wasm \
  --source deployer \
  --network testnet
```

Save as `EVENTS_CONTRACT_ID`.

**3.2.3. Rebalancing Contract**

```bash
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/astraport_rebalancing.wasm \
  --source deployer \
  --network testnet
```

Save as `REBALANCING_CONTRACT_ID`.

**3.2.4. Staking Contract**

```bash
soroban contract deploy \
  --wasm target/wasm32-unknown-unknown/release/astraport_staking.wasm \
  --source deployer \
  --network testnet
```

Save as `STAKING_CONTRACT_ID`.

### Step 3.3: Initialize Contracts

**3.3.1. Initialize Audit Contract**

```bash
soroban contract invoke \
  --id $AUDIT_CONTRACT_ID \
  --source deployer \
  --network testnet \
  -- initialize \
  --admin $ADMIN_ADDRESS
```

**3.3.2. Initialize Events Contract**

```bash
soroban contract invoke \
  --id $EVENTS_CONTRACT_ID \
  --source deployer \
  --network testnet \
  -- initialize
```

**3.3.3. Initialize Staking Contract**

```bash
soroban contract invoke \
  --id $STAKING_CONTRACT_ID \
  --source deployer \
  --network testnet \
  -- initialize \
  --admin $ADMIN_ADDRESS
```

**3.3.4. Initialize Rebalancing Contract**

```bash
soroban contract invoke \
  --id $REBALANCING_CONTRACT_ID \
  --source deployer \
  --network testnet \
  -- initialize
```

---

## 4. Mainnet Deployment

> ⚠️ **WARNING:** Mainnet deployment involves real assets. Double-check everything before proceeding.

### Step 4.1: Pre-Deployment Checklist

- [ ] All testnet tests pass
- [ ] Integration tests pass
- [ ] Security audit completed
- [ ] WASM binaries are release builds
- [ ] All contract addresses documented
- [ ] Admin keys are securely stored
- [ ] Rollback plan prepared

### Step 4.2: Configure Network

```bash
stellar network use mainnet
```

### Step 4.3: Deploy Contracts

Follow the same order as testnet (Section 3.2), replacing `--network testnet` with `--network mainnet`.

### Step 4.4: Initialize Contracts

Follow the same order as testnet (Section 3.3), replacing `--network testnet` with `--network mainnet`.

---

## 5. Post-Deployment Configuration

### 5.1: Connect Staking → Audit

```bash
soroban contract invoke \
  --id $STAKING_CONTRACT_ID \
  --source admin \
  --network testnet \
  -- set_audit_sink \
  --admin $ADMIN_ADDRESS \
  --sink $AUDIT_CONTRACT_ID
```

### 5.2: Connect Rebalancing → Audit

```bash
soroban contract invoke \
  --id $REBALANCING_CONTRACT_ID \
  --source admin \
  --network testnet \
  -- set_audit_sink \
  --sink $AUDIT_CONTRACT_ID
```

### 5.3: Configure Staking Defaults (Optional)

```bash
soroban contract invoke \
  --id $STAKING_CONTRACT_ID \
  --source admin \
  --network testnet \
  -- set_yield_defaults \
  --default_apr 50000000000000000 \
  --default_mode Daily
```

### 5.4: Configure Emergency Unstaking (Optional)

```bash
soroban contract invoke \
  --id $STAKING_CONTRACT_ID \
  --source admin \
  --network testnet \
  -- configure_emergency_unstake \
  --admin $ADMIN_ADDRESS \
  --penalty_start_bps 3000 \
  --penalty_end_bps 500 \
  --decay_function Linear \
  --cooldown_seconds 86400 \
  --treasury $TREASURY_ADDRESS \
  --enabled true
```

### 5.5: Configure Audit Retention (Optional)

```bash
soroban contract invoke \
  --id $AUDIT_CONTRACT_ID \
  --source admin \
  --network testnet \
  -- set_retention_policy \
  --admin $ADMIN_ADDRESS \
  --policy '{"max_entries":10000,"max_age_seconds":31536000}'
```

---

## 6. Deployment Order

The contracts must be deployed in this order due to dependencies:

```
1. Audit Contract (no dependencies)
2. Events Contract (no dependencies)
3. Rebalancing Contract (depends on audit)
4. Staking Contract (depends on audit)
```

After deployment:

```
5. Connect Staking → Audit (set_audit_sink)
6. Connect Rebalancing → Audit (set_audit_sink)
7. Configure defaults and policies
```

---

## 7. Verification

### 7.1: Verify Contract Deployment

```bash
# Check contract exists
soroban contract invoke --id $STAKING_CONTRACT_ID --network testnet -- get_admin
```

### 7.2: Verify Audit Integration

```bash
# Test audit sink is configured
soroban contract invoke \
  --id $STAKING_CONTRACT_ID \
  --network testnet \
  -- get_audit_sink
```

### 7.3: Run Smoke Tests

```bash
# Test basic staking flow
soroban contract invoke \
  --id $STAKING_CONTRACT_ID \
  --source testuser \
  --network testnet \
  -- stake \
  --staker $TEST_USER_ADDRESS \
  --asset XLM \
  --amount 1000000

# Verify balance
soroban contract invoke \
  --id $STAKING_CONTRACT_ID \
  --network testnet \
  -- get_balance \
  --staker $TEST_USER_ADDRESS \
  --asset XLM
```

### 7.4: Verify Integrity

```bash
# Check audit chain integrity
soroban contract invoke \
  --id $AUDIT_CONTRACT_ID \
  --network testnet \
  -- full_recompute_integrity
```

---

## 8. Rollback Plan

### If Issues Are Found Post-Deployment

1. **Disable emergency unstaking** (if configured):
   ```bash
   soroban contract invoke \
     --id $STAKING_CONTRACT_ID \
     --source admin \
     --network testnet \
     -- configure_emergency_unstake \
     --admin $ADMIN_ADDRESS \
     --penalty_start_bps 0 \
     --penalty_end_bps 0 \
     --decay_function Linear \
     --cooldown_seconds 0 \
     --treasury $TREASURY_ADDRESS \
     --enabled false
   ```

2. **Deploy new contract versions** if a critical bug is found.

3. **Contact the team** immediately if mainnet funds are at risk.

### Contract Upgradeability

AstraPort contracts do not include built-in upgrade mechanisms. If upgrades are needed:
- Deploy a new contract version.
- Migrate users to the new contract.
- The old contract remains immutable.

---

## Reference: Contract Addresses

After deployment, record all contract addresses:

| Contract | Address | Network |
|----------|---------|---------|
| Audit | `...` | testnet |
| Events | `...` | testnet |
| Rebalancing | `...` | testnet |
| Staking | `...` | testnet |

Store these securely and share them with integrators.
