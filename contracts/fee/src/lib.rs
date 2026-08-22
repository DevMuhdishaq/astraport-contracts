#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Symbol,
};

// ============================================================================
// Constants
// ============================================================================

const ADMIN: Symbol = symbol_short!("ADMIN");
const FEE_IDS: Symbol = symbol_short!("FEE_IDS");
const FEE_STRT: Symbol = symbol_short!("FEE_STRT");
const PORT_FEE: Symbol = symbol_short!("PORT_FEE");
const FEE_HIST: Symbol = symbol_short!("FEE_HIST");
const FEE_WVR: Symbol = symbol_short!("FEE_WVR");
const REV_LEDG: Symbol = symbol_short!("REV_LEDG");
const TOT_COLL: Symbol = symbol_short!("TOT_COLL");
const MAX_HISTORY: u32 = 100;
const MAX_RECIPIENTS: u32 = 20;
const BPS_DENOM: i128 = 10_000;

// ============================================================================
// Error Types
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracterror]
pub enum Error {
    FeeNotFound = 1,
    FeeInactive = 2,
    InvalidFeeConfiguration = 3,
    ArithmeticOverflow = 4,
    FeeWaiverNotFound = 5,
    TooManyRecipients = 6,
}

// ============================================================================
// Data Types
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub enum FeeType {
    Flat,
    Percentage,
    Tiered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub struct TierEntry {
    pub threshold: i128,
    pub fee_bps: i128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct FeeStructure {
    pub fee_id: Symbol,
    pub fee_type: FeeType,
    pub amount_bps: i128,
    pub tiered_entries: soroban_sdk::Vec<TierEntry>,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct RevenueRecipient {
    pub address: Address,
    pub share_numerator: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct FeeRecord {
    pub fee_id: Symbol,
    pub portfolio_id: Symbol,
    pub amount: i128,
    pub calculated_fee: i128,
    pub timestamp: u64,
    pub beneficiary: Address,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct FeeWaiver {
    pub address: Option<Address>,
    pub portfolio_id: Option<Symbol>,
    pub discount_bps: i128,
    pub waived: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct FeeCalculationResult {
    pub fee_id: Symbol,
    pub gross_amount: i128,
    pub discount_bps: i128,
    pub fee_amount: i128,
    pub waived: bool,
}

// ============================================================================
// Storage Helpers
// ============================================================================

fn get_admin(env: &Env) -> Address {
    env.storage().instance().get(&ADMIN).unwrap()
}
fn put_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&ADMIN, admin);
}
fn put_fee_structure(env: &Env, fs: &FeeStructure) {
    let key = (&FEE_STRT, &fs.fee_id);
    env.storage().persistent().set(&key, fs);
}
fn get_fee_structure(env: &Env, fee_id: &Symbol) -> Option<FeeStructure> {
    let key = (&FEE_STRT, fee_id);
    env.storage().persistent().get(&key)
}
fn add_fee_id(env: &Env, fee_id: &Symbol) {
    let mut list: soroban_sdk::Vec<Symbol> = env
        .storage()
        .persistent()
        .get(&FEE_IDS)
        .unwrap_or_else(|| soroban_sdk::Vec::new(env));
    for existing in list.iter() {
        if existing == *fee_id {
            return;
        }
    }
    list.push_back(fee_id.clone());
    env.storage().persistent().set(&FEE_IDS, &list);
}
fn list_all_fee_ids(env: &Env) -> soroban_sdk::Vec<Symbol> {
    env.storage()
        .persistent()
        .get(&FEE_IDS)
        .unwrap_or_else(|| soroban_sdk::Vec::new(env))
}
fn get_portfolio_fee_id(env: &Env, pid: &Symbol) -> Option<Symbol> {
    let key = (&PORT_FEE, pid);
    env.storage().persistent().get(&key)
}
fn set_portfolio_fee_id(env: &Env, pid: &Symbol, fid: &Symbol) {
    let key = (&PORT_FEE, pid);
    env.storage().persistent().set(&key, fid);
}
fn remove_portfolio_fee_id(env: &Env, pid: &Symbol) {
    let key = (&PORT_FEE, pid);
    env.storage().persistent().remove(&key);
}
fn get_fee_history(env: &Env) -> soroban_sdk::Vec<FeeRecord> {
    env.storage()
        .persistent()
        .get(&FEE_HIST)
        .unwrap_or_else(|| soroban_sdk::Vec::new(env))
}
fn append_fee_record(env: &Env, record: &FeeRecord) {
    let mut history = get_fee_history(env);
    if history.len() >= MAX_HISTORY {
        history = history.slice(1..);
    }
    history.push_back(record.clone());
    env.storage().persistent().set(&FEE_HIST, &history);
}
fn get_fee_waivers(env: &Env) -> soroban_sdk::Vec<FeeWaiver> {
    env.storage()
        .persistent()
        .get(&FEE_WVR)
        .unwrap_or_else(|| soroban_sdk::Vec::new(env))
}
fn put_fee_waivers(env: &Env, w: &soroban_sdk::Vec<FeeWaiver>) {
    env.storage().persistent().set(&FEE_WVR, w);
}
fn get_revenue_recipients(env: &Env) -> soroban_sdk::Vec<RevenueRecipient> {
    env.storage()
        .persistent()
        .get(&REV_LEDG)
        .unwrap_or_else(|| soroban_sdk::Vec::new(env))
}
fn put_revenue_recipients(env: &Env, r: &soroban_sdk::Vec<RevenueRecipient>) {
    env.storage().persistent().set(&REV_LEDG, r);
}
fn add_to_total_collected(env: &Env, amount: i128) {
    let cur: i128 = env.storage().instance().get(&TOT_COLL).unwrap_or(0);
    env.storage().instance().set(&TOT_COLL, &(cur + amount));
}

// ============================================================================
// Contract
// ============================================================================

#[contract]
pub struct FeeManagementContract;

#[contractimpl]
impl FeeManagementContract {
    // -- Internal Helpers --

    pub(crate) fn compute_raw_fee(
        env: &Env,
        ft: &FeeType,
        ab: &i128,
        te: &soroban_sdk::Vec<TierEntry>,
        amt: i128,
    ) -> i128 {
        match ft {
            FeeType::Flat => *ab,
            FeeType::Percentage => amt
                .checked_mul(*ab)
                .unwrap_or_else(|| soroban_sdk::panic_with_error!(env, Error::ArithmeticOverflow))
                / BPS_DENOM,
            FeeType::Tiered => Self::calculate_tiered_fee(env, te, amt),
        }
    }

    pub(crate) fn clamp_fee(fee: i128, base: i128) -> i128 {
        let f = if fee < 0 { 0 } else { fee };
        if f > base { base } else { f }
    }

    fn apply_discount(env: &Env, gf: i128, db: i128, waived: bool) -> i128 {
        if waived {
            0
        } else if db <= 0 {
            gf
        } else {
            let n = gf
                .checked_mul(BPS_DENOM - db)
                .unwrap_or_else(|| soroban_sdk::panic_with_error!(env, Error::ArithmeticOverflow))
                / BPS_DENOM;
            if n < 0 { 0 } else { n }
        }
    }

    fn calculate_tiered_fee(env: &Env, tiers: &soroban_sdk::Vec<TierEntry>, amt: i128) -> i128 {
        let mut abps: i128 = 0;
        let mut found = false;
        let len = tiers.len();
        if len == 0 {
            return 0;
        }
        let mut i = len;
        while i > 0 {
            i -= 1;
            let t = tiers.get(i).unwrap();
            if amt >= t.threshold {
                abps = t.fee_bps;
                found = true;
                break;
            }
        }
        if !found {
            return 0;
        }
        amt.checked_mul(abps)
            .unwrap_or_else(|| soroban_sdk::panic_with_error!(env, Error::ArithmeticOverflow))
            / BPS_DENOM
    }

    pub(crate) fn waiver_matches(a: &FeeWaiver, b: &FeeWaiver) -> bool {
        match (&a.address, &b.address) {
            (Some(a1), Some(a2)) => return a1 == a2,
            (None, None) => {}
            _ => return false,
        }
        match (&a.portfolio_id, &b.portfolio_id) {
            (Some(p1), Some(p2)) => p1 == p2,
            (None, None) => true,
            _ => false,
        }
    }

    fn resolve_waiver_for_portfolio(env: &Env, pid: &Symbol) -> (i128, bool) {
        for w in get_fee_waivers(env).iter() {
            if let Some(ref wp) = w.portfolio_id {
                if wp == pid {
                    return (w.discount_bps, w.waived);
                }
            }
        }
        (0, false)
    }

    fn resolve_waiver_for_collect(
        env: &Env,
        addr: &Address,
        pid: &Symbol,
    ) -> (i128, bool) {
        for w in get_fee_waivers(env).iter() {
            if let Some(ref wa) = w.address {
                if wa == addr {
                    return (w.discount_bps, w.waived);
                }
            }
            if let Some(ref wp) = w.portfolio_id {
                if wp == pid {
                    return (w.discount_bps, w.waived);
                }
            }
        }
        (0, false)
    }

    fn distribute_revenue(env: &Env, amount: i128) -> soroban_sdk::Vec<(Address, i128)> {
        let recips = get_revenue_recipients(env);
        let mut r = soroban_sdk::Vec::new(env);
        if recips.is_empty() || amount <= 0 {
            return r;
        }
        let mut ts: i128 = 0;
        for rp in recips.iter() {
            ts += rp.share_numerator as i128;
        }
        if ts == 0 {
            return r;
        }
        let mut dist: i128 = 0;
        for rp in recips.iter() {
            let s = (rp.share_numerator as i128)
                .checked_mul(amount)
                .unwrap_or_else(|| {
                    soroban_sdk::panic_with_error!(env, Error::ArithmeticOverflow)
                })
                / ts;
            dist += s;
            r.push_back((rp.address, s));
        }
        let rem = amount - dist;
        if rem > 0 && !r.is_empty() {
            let (fa, fs) = r.get(0).unwrap();
            r.set(0, (fa, fs + rem));
        }
        r
    }
}
