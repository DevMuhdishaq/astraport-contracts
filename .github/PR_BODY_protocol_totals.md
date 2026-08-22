## Summary

Adds protocol-level aggregates so dashboards and the events/AI layer can read
total value locked and the number of active stakers without scanning
storage.

Closes #19

## What's new

- `total_staked(env, asset: Symbol) -> i128` — sum of every non-zero
  `(staker, asset)` balance, maintained incrementally.
- `staker_count(env) -> u32` — count of distinct stakers with at least one
  non-zero balance across any asset.

Both are updated by:

- `stake(...)`
- `unstake(...)`
- `emergency_unstake(...)` (uses the same unstake hook, no double-decrement)

### Counter transition semantics

To handle stakers holding **multiple assets** correctly, totals are
tracked using a small piece of per-staker bookkeeping:

- `StakeDataKey::TotalStaked(Symbol)` — running aggregate per asset.
- `StakeDataKey::StakerPositionCount(Address)` — how many distinct
  `(staker, asset)` positions this staker currently holds with non-zero
  balance.
- `StakeDataKey::ActiveStakerCount` — global distinct-staker counter.

`staker_count` only flips when `StakerPositionCount` crosses `1 ↔ 0`:

- First stake ever (any asset): 0 → 1 active position ⇒ `staker_count++`.
- Subsequent stakes / different assets: positions become 2, 3… ⇒ no change.
- When a balance returns to zero: positions--
  - still >0 ⇒ `staker_count` unchanged
  - becomes 0 ⇒ `staker_count--` (full exit)

This is symmetric for restake after a full exit: a staker returning to
the protocol re-increments both `staker_count` and `total_staked`.

## Why

Dashboards, off-chain analytics, and the events/AI layer previously had
no first-class way to read TVL or staker counts without iterating all
stored balances. This makes those values O(1) reads maintained
consistently by every mutation path.

## Files changed

- `contracts/staking/src/records.rs` — new `StakeDataKey` variants:
  `TotalStaked`, `ActiveStakerCount`, `StakerPositionCount`.
- `contracts/staking/src/lib.rs`
  - New public methods `total_staked` and `staker_count`.
  - New private helpers `update_totals_on_stake` / `update_totals_on_unstake`.
  - Hooked into `stake`, `unstake`, and `emergency_unstake`.
- `contracts/staking/src/tests.rs` — 7 new tests covering the
  acceptance criteria (see below).

## Tests added

1. `test_totals_initial_state` — reads return 0 on a fresh contract.
2. `test_total_staked_reflects_stake_and_unstake_one_staker` —
   stake/partial-unstake/full-exit of a single staker, asserts both
   `total_staked` and `staker_count` track transitions to/from zero.
3. `test_total_staked_sums_across_multiple_stakers` — three stakers,
   `total_staked` matches the sum of individual balances; one staker
   exiting leaves the others' contribution intact.
4. `test_totals_are_per_asset` — XLM/USDC tracked independently;
   `staker_count` across assets behaves correctly when only one of
   several balances is unstaked.
5. `test_staker_count_increments_only_on_first_active_position` —
   second stake on the same asset (and on a different asset) does not
   re-increment `staker_count`.
6. `test_totals_update_on_emergency_unstake` — emergency_unstake path
   also keeps totals consistent (no double-decrement).
7. `test_totals_handle_re_stake_after_full_exit` — full exit followed by
   a fresh stake correctly increments `staker_count` again.

## Acceptance criteria (from #19)

- [x] `total_staked(asset)` equals the sum of all balances for an asset.
- [x] `staker_count` increments on first stake and decrements when a
  balance returns to 0.
- [x] Tests cover multiple stakers, partial unstakes, and full exits.
- [x] `cargo clippy --all-targets` is clean (no warnings).

## Notes

- Bookkeeping is O(1) per mutation; no extra storage reads.
- Helper functions are private (`impl StakingContract { fn ... }`) and
  intentionally `assert!`-based — they panic on impossible state
  divergence rather than returning a public `Error`, since this code
  path can only be reached through the contract's own mutations.

closes #19
