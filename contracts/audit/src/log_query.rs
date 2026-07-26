//! Query builder for the audit log.
//!
//! `LogQuery` accumulates optional filters; `matches()` evaluates them against
//! a single `AuditLog`. The contract entrypoint `query()` walks the
//! append-only primary index and returns every entry that matches with a
//! cap of `limit`.
//!
//! This is intentionally simple — secondary indexes are maintained by the
//! contract library but the query currently iterates the primary index to
//! keep the implementation small. The bucketed indices in [`crate::records`]
//! remain available for future optimization.

use soroban_sdk::{contracttype, Address, Symbol};

use crate::records::{AuditEventType, AuditLog};

/// Filter set for log queries.
///
/// `None` means "any" for that field; `Some(_)` narrows the search.
#[contracttype]
#[derive(Debug, Clone, Default)]
pub struct LogQuery {
    /// Inclusive lower bound on `entry.timestamp`. `0` is treated as unset.
    pub from_ts: u64,
    /// Inclusive upper bound on `entry.timestamp`. `0` is treated as unset.
    pub to_ts: u64,
    /// Optional event-type filter.
    pub event_type: Option<AuditEventType>,
    /// Optional actor filter.
    pub actor: Option<Address>,
    /// Optional portfolio filter.
    pub portfolio: Option<Symbol>,
    /// Maximum number of entries returned.
    pub limit: u32,
    /// Reserved for future use (e.g. cursor-based pagination).
    pub cursor: u64,
}

impl LogQuery {
    /// Build an empty query with a default limit.
    pub fn new(limit: u32) -> Self {
        Self {
            from_ts: 0,
            to_ts: 0,
            event_type: None,
            actor: None,
            portfolio: None,
            limit,
            cursor: 0,
        }
    }

    /// Restrict the query to events at or after this timestamp.
    pub fn from_ts(mut self, ts: u64) -> Self {
        self.from_ts = ts;
        self
    }

    /// Restrict the query to events at or before this timestamp.
    pub fn to_ts(mut self, ts: u64) -> Self {
        self.to_ts = ts;
        self
    }

    /// Restrict the query to a single event type.
    pub fn event_type(mut self, t: AuditEventType) -> Self {
        self.event_type = Some(t);
        self
    }

    /// Restrict the query to a single actor.
    pub fn actor(mut self, a: Address) -> Self {
        self.actor = Some(a);
        self
    }

    /// Restrict the query to a single portfolio.
    pub fn portfolio(mut self, p: Symbol) -> Self {
        self.portfolio = Some(p);
        self
    }

    /// Override the maximum number of entries returned.
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = limit;
        self
    }

    /// Return `true` when `entry` satisfies every active filter.
    pub fn matches(&self, entry: &AuditLog) -> bool {
        if self.from_ts != 0 && entry.timestamp < self.from_ts {
            return false;
        }
        if self.to_ts != 0 && entry.timestamp > self.to_ts {
            return false;
        }
        if let Some(t) = &self.event_type {
            if entry.event_type != *t {
                return false;
            }
        }
        if let Some(a) = &self.actor {
            // Compare via the SDK's address stringification (sufficient for
            // exact equality on Soroban contract addresses).
            if entry.actor.to_string() != a.to_string() {
                return false;
            }
        }
        if let Some(p) = &self.portfolio {
            if entry.portfolio != *p {
                return false;
            }
        }
        true
    }
}
