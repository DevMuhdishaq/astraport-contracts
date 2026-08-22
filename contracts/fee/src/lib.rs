#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Symbol,
};

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
// Contract
// ============================================================================

#[contract]
pub struct FeeManagementContract;

#[contractimpl]
impl FeeManagementContract {
    pub fn initialize(_env: Env, _admin: Address) -> Symbol {
        symbol_short!("ok")
    }
}
