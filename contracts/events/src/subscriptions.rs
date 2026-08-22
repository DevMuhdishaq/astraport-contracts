// ---------------------------------------------------------------------------
// Advanced event subscription system (issue #7)
//
// Filtering by type/severity/custom predicates, delivery preferences
// (immediate vs batch), acknowledgment tracking, exponential-backoff retry
// scheduling and delivery metrics. Pure logic lives here; the contract entry
// points wire it to storage in lib.rs.
// ---------------------------------------------------------------------------

use crate::{ComparisonOperator, PortfolioEventType};
use soroban_sdk::{contracttype, Env, Symbol, Vec};

/// Retry backoff configuration: initial 1s, doubling, capped at 1h.
pub const RETRY_INITIAL_SECS: u64 = 1;
pub const RETRY_MAX_SECS: u64 = 3_600;

/// Delivery modes for a managed subscription.
#[contracttype]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum DeliveryMode {
    /// Deliver each matching event as soon as it is dispatched.
    Immediate = 0,
    /// Accumulate events; flush when batch size or window is reached.
    Batch = 1,
}

/// Lifecycle status of a managed subscription.
#[contracttype]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SubscriptionStatus {
    Active = 0,
    Paused = 1,
    Cancelled = 2,
}

impl SubscriptionStatus {
    pub fn is_receives_events(self) -> bool {
        matches!(self, SubscriptionStatus::Active)
    }
}

/// A single custom predicate evaluated against an event's `details` map.
///
/// The detail value must be a `Bytes` payload of exactly 16 bytes encoding a
/// big-endian i128 (see [`numeric_detail`] for writing one). Non-numeric
/// details never satisfy a condition.
#[contracttype]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterCondition {
    pub key: Symbol,
    pub op: ComparisonOperator,
    pub value: i128,
}

/// Filtering criteria applied to every dispatched event.
///
/// An empty `event_types` vector means "all types"; `min_severity` of 0 means
/// "any severity"; ALL `conditions` must pass (empty = no extra constraint).
#[contracttype]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventFilter {
    pub event_types: Vec<PortfolioEventType>,
    pub min_severity: u32,
    pub conditions: Vec<FilterCondition>,
}

impl EventFilter {
    pub fn matches(
        &self,
        env: &Env,
        event_type: &PortfolioEventType,
        severity: u32,
        details: &soroban_sdk::Map<Symbol, soroban_sdk::Bytes>,
    ) -> bool {
        if self.min_severity > severity {
            return false;
        }
        if !self.event_types.is_empty() && !contains_type(&self.event_types, event_type) {
            return false;
        }
        for i in 0..self.conditions.len() {
            let cond = self.conditions.get(i).unwrap();
            if !condition_matches(env, &cond, details) {
                return false;
            }
        }
        true
    }
}

fn contains_type(types: &Vec<PortfolioEventType>, t: &PortfolioEventType) -> bool {
    for i in 0..types.len() {
        if types.get(i).unwrap() == *t {
            return true;
        }
    }
    false
}

/// Encode an i128 as the 16-byte big-endian detail payload expected by
/// [`FilterCondition`].
pub fn numeric_detail(env: &Env, value: i128) -> soroban_sdk::Bytes {
    let be = value.to_be_bytes();
    let mut b = soroban_sdk::Bytes::new(env);
    for byte in be {
        b.push_back(byte);
    }
    b
}

fn condition_matches(
    env: &Env,
    cond: &FilterCondition,
    details: &soroban_sdk::Map<Symbol, soroban_sdk::Bytes>,
) -> bool {
    let raw = match details.get(cond.key.clone()) {
        Some(r) => r,
        None => return false,
    };
    if raw.len() != 16 {
        return false;
    }
    let mut buf = [0u8; 16];
    for (i, byte) in raw.iter().enumerate() {
        buf[i] = byte;
    }
    let actual = i128::from_be_bytes(buf);
    let _ = env;
    match cond.op {
        ComparisonOperator::GreaterThan => actual > cond.value,
        ComparisonOperator::LessThan => actual < cond.value,
        ComparisonOperator::EqualTo => actual == cond.value,
        ComparisonOperator::GreaterOrEqual => actual >= cond.value,
        ComparisonOperator::LessOrEqual => actual <= cond.value,
    }
}

/// Per-subscription notification preferences.
#[contracttype]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionPreferences {
    pub mode: DeliveryMode,
    /// Batch mode: flush automatically when this many events accumulate.
    /// Ignored in immediate mode.
    pub batch_size: u32,
    /// Batch mode: maximum seconds an event may wait in the batch.
    pub batch_window_secs: u64,
}

impl SubscriptionPreferences {
    pub fn immediate() -> Self {
        Self {
            mode: DeliveryMode::Immediate,
            batch_size: 1,
            batch_window_secs: 0,
        }
    }

    pub fn batch(batch_size: u32, window_secs: u64) -> Self {
        Self {
            mode: DeliveryMode::Batch,
            batch_size: batch_size.max(1),
            batch_window_secs: window_secs,
        }
    }
}

/// Fully-featured subscription with metadata, managed by the contract.
#[contracttype]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedSubscription {
    pub id: u64,
    pub subscriber: soroban_sdk::Address,
    pub portfolio_id: Symbol,
    pub filter: EventFilter,
    pub prefs: SubscriptionPreferences,
    pub created_at: u64,
    /// Timestamp of the last event matched for this subscription.
    pub last_event_received_at: Option<u64>,
    pub status: SubscriptionStatus,
    pub total_delivered: u32,
    pub total_failed: u32,
}

/// Delivery lifecycle states.
#[contracttype]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum DeliveryStatus {
    /// Scheduled; waiting for `next_retry_at` before first/next attempt.
    Pending = 0,
    /// NOTIFY published; awaiting subscriber acknowledgment.
    Delivered = 1,
    /// Subscriber acknowledged receipt.
    Acknowledged = 2,
    /// Exhausted or explicitly failed; no further retries scheduled.
    Failed = 3,
}

/// Tracking record for one event -> one subscription delivery.
#[contracttype]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryRecord {
    pub event_id: u64,
    pub subscription_id: u64,
    pub subscriber: soroban_sdk::Address,
    pub status: DeliveryStatus,
    /// Number of delivery attempts made so far.
    pub attempts: u32,
    /// Earliest ledger timestamp at which the next attempt may run.
    pub next_retry_at: u64,
    pub delivered_at: Option<u64>,
    pub acked_at: Option<u64>,
}

/// Aggregate delivery metrics for observability.
#[contracttype]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryMetrics {
    pub total_dispatched: u32,
    pub total_delivered: u32,
    pub total_acknowledged: u32,
    pub total_failed: u32,
    pub total_retry_attempts: u32,
    /// Sum of (acked_at - event timestamp) over acknowledgments, in seconds.
    pub latency_sum_secs: u64,
}

impl DeliveryMetrics {
    #[must_use]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            total_dispatched: 0,
            total_delivered: 0,
            total_acknowledged: 0,
            total_failed: 0,
            total_retry_attempts: 0,
            latency_sum_secs: 0,
        }
    }

    /// Successful deliveries / attempts, in basis points (0..=10000).
    /// Returns 10000 when no attempts have been made yet.
    pub fn success_rate_bps(&self) -> u32 {
        if self.total_delivered + self.total_failed == 0 {
            return 10_000;
        }
        ((self.total_delivered as u128 * 10_000)
            / (self.total_delivered + self.total_failed) as u128) as u32
    }
}

/// Compute the delay before the attempt numbered `attempt` (0-based count of
/// PRIOR attempts). Exponential with saturation at [`RETRY_MAX_SECS`].
pub fn backoff_delay(attempt: u32) -> u64 {
    let mut delay = RETRY_INITIAL_SECS;
    let shifts = attempt.min(31);
    for _ in 0..shifts {
        let doubled = delay.saturating_mul(2);
        delay = doubled.min(RETRY_MAX_SECS);
        if delay >= RETRY_MAX_SECS {
            break;
        }
    }
    delay.min(RETRY_MAX_SECS)
}
