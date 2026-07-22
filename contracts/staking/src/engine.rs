//! The storage-backed yield engine.
//!
//! [`YieldEngine`] is the stateful layer that sits on top of the pure math in
//! [`crate::compounding`] / [`crate::apy`] and the projection helper in
//! [`crate::projection`]. It owns the persistence of [`YieldRecord`]s,
//! append-only [`YieldHistoryEntry`] logs, and [`DistributionSchedule`]s, and it
//! implements **real-time accrual**: yield is checkpointed against the ledger
//! clock so a position's earnings are always current when queried.
//!
//! Rate changes are handled by checkpointing accrued yield at the old rate
//! *before* applying the new rate, which makes the history time-weighted and
//! exact across rate boundaries.

use soroban_sdk::{Address, Env, Symbol, Vec};

use crate::compounding::YieldCalculator;
use crate::fixed_point::MathError;
use crate::records::{
    CompoundingMode, DistributionSchedule, YieldDataKey, YieldHistoryEntry, YieldRecord,
};

/// Stateful engine coordinating yield accrual, history, and distributions.
///
/// The engine is a thin, `env`-scoped facade; it holds no state itself beyond
/// the borrowed [`Env`] and reads/writes everything through persistent storage.
pub struct YieldEngine<'a> {
    env: &'a Env,
}

impl<'a> YieldEngine<'a> {
    /// Create an engine bound to the given environment.
    pub fn new(env: &'a Env) -> Self {
        Self { env }
    }

    /// Open or create a position for `(staker, asset)` with the given principal,
    /// rate, and compounding mode.
    ///
    /// If a record already exists it is first brought current (accrued to now)
    /// and then overwritten with the new principal/rate, preserving accrued
    /// yield. The starting timestamp is the current ledger time.
    pub fn open_position(
        &self,
        staker: &Address,
        asset: &Symbol,
        principal: i128,
        apr: i128,
        mode: CompoundingMode,
    ) -> Result<YieldRecord, MathError> {
        let now = self.env.ledger().timestamp();

        // If a position exists, checkpoint it first so nothing is lost.
        let accrued = match self.load_record(staker, asset) {
            Some(existing) => {
                let updated = self.accrue_to(&existing, now)?;
                updated.accrued_yield
            }
            None => 0,
        };

        let record = YieldRecord {
            staker: staker.clone(),
            asset: asset.clone(),
            principal,
            apr,
            mode,
            last_accrual_ts: now,
            accrued_yield: accrued,
        };
        self.store_record(&record);
        Ok(record)
    }

    /// Bring a position's accrued yield current as of `now`, appending a history
    /// entry for the elapsed period and persisting the updated record.
    ///
    /// Returns the updated record. Accruing to a timestamp at or before the last
    /// checkpoint is a no-op (returns the record unchanged) so repeated calls in
    /// the same ledger are safe.
    pub fn accrue(&self, staker: &Address, asset: &Symbol) -> Result<YieldRecord, MathError> {
        let now = self.env.ledger().timestamp();
        let record = self
            .load_record(staker, asset)
            .ok_or(MathError::NegativeInput)?; // treat "missing" as invalid input
        let updated = self.accrue_to(&record, now)?;
        self.store_record(&updated);
        Ok(updated)
    }

    /// Adjust the principal of an existing position to `new_principal`,
    /// preserving its APR, compounding mode, and realized `accrued_yield`.
    ///
    /// The position is checkpointed (accrued to now) *before* the principal
    /// changes, so all yield earned on the old principal is realized and no yield
    /// is lost across the boundary. This is what stake/unstake use to keep a
    /// position's principal equal to the staked balance without resetting its
    /// rate.
    pub fn set_principal(
        &self,
        staker: &Address,
        asset: &Symbol,
        new_principal: i128,
    ) -> Result<YieldRecord, MathError> {
        let now = self.env.ledger().timestamp();
        let record = self
            .load_record(staker, asset)
            .ok_or(MathError::NegativeInput)?;
        // Realize everything earned on the old principal first.
        let mut updated = self.accrue_to(&record, now)?;
        // Then move principal going forward.
        updated.principal = new_principal;
        self.store_record(&updated);
        Ok(updated)
    }

    /// Change the APR for a position, checkpointing accrued yield at the old rate
    /// first so the transition is exact and time-weighted.
    pub fn set_rate(
        &self,
        staker: &Address,
        asset: &Symbol,
        new_apr: i128,
    ) -> Result<YieldRecord, MathError> {
        let now = self.env.ledger().timestamp();
        let record = self
            .load_record(staker, asset)
            .ok_or(MathError::NegativeInput)?;
        // Accrue everything earned at the OLD rate up to now.
        let mut updated = self.accrue_to(&record, now)?;
        // Then swap in the new rate going forward.
        updated.apr = new_apr;
        self.store_record(&updated);
        Ok(updated)
    }

    /// The total yield a position has earned *right now* — checkpointed accrued
    /// yield plus the yet-uncheckpointed amount since the last accrual.
    ///
    /// This is a read-only view; it does not mutate storage.
    pub fn current_yield(&self, staker: &Address, asset: &Symbol) -> Result<i128, MathError> {
        let now = self.env.ledger().timestamp();
        let record = self
            .load_record(staker, asset)
            .ok_or(MathError::NegativeInput)?;
        let pending = self.pending_yield(&record, now)?;
        record
            .accrued_yield
            .checked_add(pending)
            .ok_or(MathError::Overflow)
    }

    /// Compute yield earned since `record.last_accrual_ts` up to `now`, without
    /// mutating anything. Yield compounds on principal only (realized yield is
    /// tracked separately, matching a claim-based staking model).
    fn pending_yield(&self, record: &YieldRecord, now: u64) -> Result<i128, MathError> {
        if now <= record.last_accrual_ts {
            return Ok(0);
        }
        let elapsed = now - record.last_accrual_ts;
        let calc = YieldCalculator::new(record.mode.to_strategy());
        calc.compute_yield(record.principal, record.apr, elapsed)
    }

    /// Internal: produce a record advanced to `now`, appending a history entry
    /// for the elapsed period. Does not persist — callers store the result.
    fn accrue_to(&self, record: &YieldRecord, now: u64) -> Result<YieldRecord, MathError> {
        if now <= record.last_accrual_ts {
            return Ok(record.clone());
        }
        let elapsed = now - record.last_accrual_ts;
        let earned = self.pending_yield(record, now)?;
        let cumulative = record
            .accrued_yield
            .checked_add(earned)
            .ok_or(MathError::Overflow)?;

        // Append to the history log.
        self.append_history(
            &record.staker,
            &record.asset,
            YieldHistoryEntry {
                timestamp: now,
                period_seconds: elapsed,
                apr: record.apr,
                yield_earned: earned,
                cumulative_yield: cumulative,
            },
        );

        let mut updated = record.clone();
        updated.accrued_yield = cumulative;
        updated.last_accrual_ts = now;
        Ok(updated)
    }

    // --- history ---------------------------------------------------------

    /// The full yield history for a `(staker, asset)` pair, oldest first.
    pub fn history(&self, staker: &Address, asset: &Symbol) -> Vec<YieldHistoryEntry> {
        let key = YieldDataKey::History(staker.clone(), asset.clone());
        self.env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(self.env))
    }

    /// Append an entry to a pair's history log.
    fn append_history(&self, staker: &Address, asset: &Symbol, entry: YieldHistoryEntry) {
        let key = YieldDataKey::History(staker.clone(), asset.clone());
        let mut log: Vec<YieldHistoryEntry> = self
            .env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(self.env));
        log.push_back(entry);
        self.env.storage().persistent().set(&key, &log);
    }

    // --- distribution scheduling ----------------------------------------

    /// Schedule a yield distribution for `(staker, asset)`.
    ///
    /// `interval_seconds` of 0 makes it a one-off; otherwise the schedule is
    /// recurring and [`Self::process_distribution`] rolls `due_ts` forward by the
    /// interval each time it fires.
    pub fn schedule_distribution(
        &self,
        staker: &Address,
        asset: &Symbol,
        amount: i128,
        due_ts: u64,
        interval_seconds: u64,
    ) -> DistributionSchedule {
        let schedule = DistributionSchedule {
            staker: staker.clone(),
            asset: asset.clone(),
            due_ts,
            interval_seconds,
            amount,
            executed: false,
        };
        let key = YieldDataKey::Schedule(staker.clone(), asset.clone());
        let mut list: Vec<DistributionSchedule> = self
            .env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(self.env));
        list.push_back(schedule.clone());
        self.env.storage().persistent().set(&key, &list);
        schedule
    }

    /// All distribution schedules for a `(staker, asset)` pair.
    pub fn schedules(&self, staker: &Address, asset: &Symbol) -> Vec<DistributionSchedule> {
        let key = YieldDataKey::Schedule(staker.clone(), asset.clone());
        self.env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(self.env))
    }

    /// Process due distributions for a pair as of the current ledger time.
    ///
    /// Returns the total amount marked due. One-off schedules are marked
    /// `executed`; recurring schedules have their `due_ts` advanced by their
    /// interval and remain active.
    pub fn process_distribution(&self, staker: &Address, asset: &Symbol) -> i128 {
        let now = self.env.ledger().timestamp();
        let key = YieldDataKey::Schedule(staker.clone(), asset.clone());
        let list: Vec<DistributionSchedule> = match self.env.storage().persistent().get(&key) {
            Some(l) => l,
            None => return 0,
        };

        let mut total: i128 = 0;
        let mut updated: Vec<DistributionSchedule> = Vec::new(self.env);
        for i in 0..list.len() {
            let mut s = list.get(i).unwrap();
            if !s.executed && s.due_ts <= now {
                total += s.amount;
                if s.interval_seconds > 0 {
                    // Recurring: advance to the next due time, stay active.
                    s.due_ts += s.interval_seconds;
                } else {
                    s.executed = true;
                }
            }
            updated.push_back(s);
        }
        self.env.storage().persistent().set(&key, &updated);
        total
    }

    // --- record persistence ---------------------------------------------

    /// Load the active record for a pair, if any.
    pub fn load_record(&self, staker: &Address, asset: &Symbol) -> Option<YieldRecord> {
        let key = YieldDataKey::Record(staker.clone(), asset.clone());
        self.env.storage().persistent().get(&key)
    }

    /// Persist a record.
    fn store_record(&self, record: &YieldRecord) {
        let key = YieldDataKey::Record(record.staker.clone(), record.asset.clone());
        self.env.storage().persistent().set(&key, record);
    }
}
