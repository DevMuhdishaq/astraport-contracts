# AstraPort Smart Contracts — Integration Guide

> Step-by-step guide for developers integrating with the AstraPort contract system.

---

## Table of Contents

1. [Prerequisites](#1-prerequisites)
2. [Project Setup](#2-project-setup)
3. [Connecting to Contracts](#3-connecting-to-contracts)
4. [Workflow 1: Portfolio Rebalancing](#4-workflow-1-portfolio-rebalancing)
5. [Workflow 2: Asset Staking with Yield](#5-workflow-2-asset-staking-with-yield)
6. [Workflow 3: Emergency Unstaking](#6-workflow-3-emergency-unstaking)
7. [Workflow 4: Event-Driven AI Analysis](#7-workflow-4-event-driven-ai-analysis)
8. [Workflow 5: Audit Logging](#8-workflow-5-audit-logging)
9. [Cross-Contract Integration](#9-cross-contract-integration)
10. [Best Practices](#10-best-practices)

---

## 1. Prerequisites

Before integrating, ensure you have:

- **Rust** 1.75.0+
- **Soroban CLI** v21.5.0+
- **Stellar testnet account** with test XLM
- **Node.js** 18+ (optional, for client-side utilities)

Install Soroban CLI:

```bash
cargo install soroban-cli
```

Add the WASM target:

```bash
rustup target add wasm32-unknown-unknown
```

---

## 2. Project Setup

### Clone and Build

```bash
git clone https://github.com/redux-space/astraport-contracts.git
cd astraport-contracts
cargo build
```

### Run Tests

```bash
cargo test
```

### Build WASM for Deployment

```bash
soroban contract build --package astraport-rebalancing
soroban contract build --package astraport-events
soroban contract build --package astraport-staking
soroban contract build --package astraport-audit
```

---

## 3. Connecting to Contracts

### Using Soroban CLI

After deployment, you'll receive contract addresses. Interact via the CLI:

```bash
# Initialize staking contract
soroban contract invoke \
  --id <STAKING_CONTRACT_ADDRESS> \
  --source <YOUR_KEY> \
  --network testnet \
  -- initialize \
  --admin <ADMIN_ADDRESS>

# Stake assets
soroban contract invoke \
  --id <STAKING_CONTRACT_ADDRESS> \
  --source <STAKER_KEY> \
  --network testnet \
  -- stake \
  --staker <STAKER_ADDRESS> \
  --asset XLM \
  --amount 10000000
```

### Using Rust Client (in-contract)

```rust
use astraport_staking::StakingContractClient;

let client = StakingContractClient::new(&env, &staking_contract_address);
let balance = client.get_balance(&staker_address, &symbol_short!("XLM"));
```

---

## 4. Workflow 1: Portfolio Rebalancing

### Step-by-Step

**4.1. Initialize the Rebalancing Contract**

```rust
let client = RebalancingContractClient::new(&env, &rebalancing_address);
client.initialize();
```

**4.2. Define Your Target Allocation**

Weights must be in basis points (10,000 = 100%) and sum to exactly 10,000.

```rust
let mut allocations = Map::new(&env);
allocations.set(symbol_short!("USDC"), 4_000); // 40%
allocations.set(symbol_short!("XLM"), 3_000);  // 30%
allocations.set(symbol_short!("BTC"), 3_000);  // 30%

let target = TargetAllocation { allocations };
```

**4.3. Set the Target Allocation**

```rust
let result = client.set_target_allocation(&owner, &portfolio_id, &target);
// First caller becomes the portfolio owner
```

**4.4. Record Current Holdings**

```rust
let mut current = Map::new(&env);
current.set(symbol_short!("USDC"), 5_000); // Currently 50%
current.set(symbol_short!("XLM"), 2_000);  // Currently 20%
current.set(symbol_short!("BTC"), 3_000);  // Currently 30%

let holdings = CurrentHoldings { allocations: current };
client.set_current_holdings(&owner, &portfolio_id, &holdings);
```

**4.5. Execute Rebalancing**

```rust
let result = client.rebalance(&owner, &portfolio_id)?;

// result.adjustments contains:
// - USDC: drift +1000 bps → Sell
// - XLM:  drift -1000 bps → Buy
```

**4.6. Set Up Automatic Rebalancing (Optional)**

```rust
client.set_schedule(&owner, &portfolio_id, &RebalanceInterval::Daily);
```

**4.7. Check Scheduled Execution**

```rust
// Called by a keeper bot when the interval elapses
let outcome = client.check_exec_sched_rebalance(&portfolio_id);
// Returns "done", "not_due", "no_target", etc.
```

---

## 5. Workflow 2: Asset Staking with Yield

### Step-by-Step

**5.1. Initialize the Staking Contract**

```rust
let client = StakingContractClient::new(&env, &staking_address);
client.initialize(&admin_address);
```

**5.2. Configure Default Yield Parameters (Optional)**

```rust
client.set_yield_defaults(
    &50_000_000_000_000_000, // 5% APR
    &CompoundingMode::Daily,
);
```

**5.3. Stake Assets**

```rust
let xlm = symbol_short!("XLM");
let amount = 10_000_000_000; // 1,000 XLM in stroops

client.stake(&staker_address, &xlm, &amount)?;
```

**5.4. Open a Yield Position**

```rust
let apr = 50_000_000_000_000_000; // 5%
let record = client.open_yield_position(
    &staker_address,
    &xlm,
    &amount,
    &apr,
    &CompoundingMode::Daily,
);
```

**5.5. Query Yield Over Time**

```rust
// After some time passes...
let yield_amount = client.current_yield(&staker_address, &xlm);
println!("Accrued yield: {} base units", yield_amount);
```

**5.6. Claim Yield**

```rust
let claimed = client.claim_yield(&staker_address, &xlm);
println!("Claimed: {} base units", claimed);
```

**5.7. Project Future Earnings**

```rust
let projection = client.project_yield(
    &10_000_000_000,  // principal
    &apr,             // 5% APR
    &CompoundingMode::Daily,
    &(90 * 86400),    // 90-day horizon
);

println!("Projected yield: {} in 90 days", projection.projected_yield);
println!("Effective APY: {}", projection.effective_apy);
```

---

## 6. Workflow 3: Emergency Unstaking

### Step-by-Step

**6.1. Configure Emergency Unstaking (Admin)**

```rust
client.configure_emergency_unstake(
    &admin,
    &3_000,    // 30% penalty at lock start
    &500,      // 5% penalty at unlock
    &PenaltyDecayFunction::Linear,
    &86_400,   // 1-day cooldown
    &treasury_address,
    &true,
);
```

**6.2. Set Lock Position (Admin)**

```rust
let now = env.ledger().timestamp();
client.set_lock_position(
    &admin,
    &staker,
    &now,                // lock_start_ts
    &(now + 30 * 86400), // unlock_ts (30 days)
    &1_000_000,          // locked_amount
);
```

**6.3. Preview Penalty Before Emergency Unstake**

```rust
let penalty_bps = client.preview_emergency_penalty(&lock_start_ts, &unlock_ts);
// Returns penalty in basis points (e.g., 2500 = 25%)
```

**6.4. Execute Emergency Unstake**

```rust
let record = client.emergency_unstake(&staker, &xlm, &500_000);

println!("Requested: {}", record.amount_requested);
println!("Penalty: {} ({} bps)", record.penalty_amount, record.penalty_bps_applied);
println!("Returned: {}", record.amount_returned);
```

**6.5. Check Cooldown Status**

```rust
let in_cooldown = client.is_in_cooldown(&staker);
let cooldown_end = client.get_cooldown_end(&staker);
```

---

## 7. Workflow 4: Event-Driven AI Analysis

### Step-by-Step

**7.1. Initialize Events Contract**

```rust
let client = EventsContractClient::new(&env, &events_address);
client.initialize();
```

**7.2. Create an AI Trigger**

```rust
let trigger = AITrigger {
    trigger_id: symbol_short!("VOLATILITY"),
    name: symbol_short!("VolatilityAlert"),
    event_types: Vec::from_array(&env, &[EventType::VolatilitySpike as u32]),
    threshold: Some(U256::from_u32(&env, 1000)), // 10% threshold
    operator: Some(ComparisonOperator::GreaterThan as u32),
    ai_service_endpoint: ai_service_address,
    timeout: 5000, // 5 second timeout
    is_active: true,
    owner: trigger_owner,
};

client.add_trigger(&trigger)?;
```

**7.3. Subscribe to Portfolio Events**

```rust
client.subscribe(&portfolio_id, &subscriber_address)?;
```

**7.4. Process an Event**

```rust
let event_type = EventType::VolatilitySpike as u32;
let event_data = Bytes::new(&env); // Your event payload
let current_value = Some(U256::from_u32(&env, 1500)); // 15%

let analysis_ids = client.process_event(
    &portfolio_id,
    &event_type,
    &event_data,
    &current_value,
)?;

// analysis_ids contains IDs of triggered analyses
```

**7.5. Update Analysis Status (by AI service)**

```rust
client.update_analysis_status(
    &analysis_id,
    &(AnalysisStatus::Completed as u32),
    &Some(2000u64),     // latency_ms
    &Some(raw_output),  // AI result bytes
    &None,              // no error
);
```

**7.6. Accept/Reject Recommendations**

```rust
client.process_recommendation_feedback(
    &recommendation_id,
    &true,  // accepted
    &responder_address,
)?;
```

---

## 8. Workflow 5: Audit Logging

### Step-by-Step

**8.1. Initialize Audit Contract**

```rust
let audit_client = AuditContractClient::new(&env, &audit_address);
audit_client.initialize(&admin_address);
```

**8.2. Connect Other Contracts to Audit Sink**

```rust
// In staking contract:
staking_client.set_audit_sink(&admin, &audit_address)?;

// In rebalancing contract:
rebalancing_client.set_audit_sink(&audit_address)?;
```

**8.3. Query Audit Logs**

```rust
let query = LogQuery::new(100)
    .event_type(AuditEventType::Stake)
    .portfolio(symbol_short!("XLM"))
    .from_ts(1_700_000_000);

let entries = audit_client.query(&query);
```

**8.4. Verify Integrity**

```rust
let head = audit_client.integrity_head();
let is_valid = audit_client.verify_integrity(&head);
assert!(is_valid, "Chain integrity should hold");

// Full recompute for routine audits
assert!(audit_client.full_recompute_integrity());
```

**8.5. Export Logs**

```rust
// JSON-Lines
let jsonl = audit_client.export_jsonl(&query);

// CSV
let csv = audit_client.export_csv(&query);
```

**8.6. Configure Retention Policy (Optional)**

```rust
let policy = RetentionPolicy {
    max_entries: 10_000,     // Keep at most 10k entries
    max_age_seconds: 86400 * 365, // 1 year
};
audit_client.set_retention_policy(&admin, &policy)?;

// Prune old entries
let pruned = audit_client.prune_old(&admin)?;
```

---

## 9. Cross-Contract Integration

### Connecting Staking → Audit

The staking contract automatically logs events to the audit contract when an audit sink is configured:

```rust
// 1. Deploy audit contract
let audit_address = deploy_audit_contract(&env, &admin);

// 2. Configure staking to use audit sink
staking_client.set_audit_sink(&admin, &audit_address)?;

// 3. Now every stake/unstake/emergency_unstake is automatically audited
staking_client.stake(&staker, &xlm, &amount)?;
// → Audit log entry created with Stake event type
```

### Connecting Rebalancing → Events

After a rebalance, emit events to the events contract:

```rust
// 1. Execute rebalance
let result = rebalancing_client.rebalance(&owner, &portfolio_id)?;

// 2. Manually trigger event processing (or use a keeper)
events_client.process_event(
    &portfolio_id,
    &(EventType::PortfolioRebalance as u32),
    &event_data,
    &None,
)?;
```

### Full Pipeline: Stake → Yield → Audit

```rust
// 1. Stake
staking_client.stake(&staker, &xlm, &1_000_000)?;

// 2. Open yield position
staking_client.open_yield_position(&staker, &xlm, &1_000_000, &apr, &mode)?;

// 3. Advance time and accrue
env.ledger().set_timestamp(now + 86400);
let record = staking_client.accrue_yield(&staker, &xlm);

// 4. Claim yield
let claimed = staking_client.claim_yield(&staker, &xlm);

// 5. All events are audited automatically (if sink configured)
```

---

## 10. Best Practices

### Authorization

- Always call `require_auth()` before state-changing operations in your own contracts.
- The rebalancing contract enforces portfolio ownership automatically.
- The staking contract requires staker authorization for stake/unstake/claim.

### Error Handling

- Use `try_*` methods in tests to inspect error variants.
- Handle `Result` types explicitly — never silently ignore errors.
- Check balance before unstaking to avoid `InsufficientBalance` errors.

### Yield Management

- Call `accrue_yield` before querying `current_yield` for the most accurate reading.
- Use `project_yield` to estimate earnings before committing to a position.
- Use `apr_to_apy` / `apy_to_apr` to communicate rates in user-friendly terms.

### Audit Trail

- Configure audit sinks on all contracts for a complete on-chain audit trail.
- Use `verify_integrity` in keeper bots to detect tampering.
- Set retention policies to manage storage costs.

### Performance

- Avoid calling `full_recompute_integrity` in hot paths — it iterates all entries.
- Use `LogQuery` filters to limit query results.
- Cache frequently-read values off-chain when possible.

### Security

- See [SECURITY.md](SECURITY.md) for comprehensive security guidelines.
- Never expose private keys in client-side code.
- Test thoroughly on testnet before mainnet deployment.
