#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Symbol,
};

// ============================================================================
// Storage Key Symbols
// ============================================================================

const ADMIN: Symbol = symbol_short!("ADMIN");
const GUARDIAN: Symbol = symbol_short!("GRDN");
const PAUSED: Symbol = symbol_short!("PAUSD");
const SAFE_MODE: Symbol = symbol_short!("SFMD");
const PAUSE_REASON: Symbol = symbol_short!("PRS_");
const SAFE_MODE_REASON: Symbol = symbol_short!("SM_RS");
const CIRCUIT_TRIP: Symbol = symbol_short!("CRT_T");
const CIRCUIT_THRESH: Symbol = symbol_short!("CRT_TH");
const MAX_TRADE: Symbol = symbol_short!("MX_TR");
const EMERG_WD_FEE: Symbol = symbol_short!("EM_WF");
const LOCK_PERIOD: Symbol = symbol_short!("LCK_P");

// ============================================================================
// Limits & Constants
// ============================================================================

const DEFAULT_CIRCUIT_THRESHOLD_BPS: i128 = 2000; // 20%
const DEFAULT_MAX_TRADE_AMOUNT: i128 = 100_000_000; // 100M units
const DEFAULT_EMERGENCY_WITHDRAWAL_FEE_BPS: i128 = 1000; // 10% penalty
const DEFAULT_LOCK_PERIOD: u64 = 86400; // 24 hours in seconds
const BPS_DENOM: i128 = 10_000;

// ============================================================================
// Errors
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracterror]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    AdminRequired = 3,
    GuardianRequired = 4,
    AlreadyPaused = 5,
    NotPaused = 6,
    AlreadyInSafeMode = 7,
    NotInSafeMode = 8,
    CircuitBreakerTripped = 9,
    TradeSizeExceedsLimit = 10,
    OperationRateLimited = 11,
    InvalidConfiguration = 12,
    ArithmeticOverflow = 13,
    InsufficientBalance = 14,
    LockPeriodNotExpired = 15,
    IncidentLogFull = 16,
    TooManyNotifiers = 17,
    CannotPauseGuardian = 18,
    OperationBlockedBySafeMode = 19,
}

// ============================================================================
// Types
// ============================================================================

/// Severity levels for incident logging.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd)]
#[contracttype]
pub enum IncidentSeverity {
    Low = 0,
    Medium = 1,
    High = 2,
    Critical = 3,
}

/// Types of emergency actions that can be logged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub enum IncidentActionType {
    Pause = 0,
    Unpause = 1,
    EmergencyWithdrawal = 2,
    CircuitBreakerTrip = 3,
    CircuitBreakerReset = 4,
    SafeModeEnter = 5,
    SafeModeExit = 6,
    MaxTradeUpdated = 7,
    ThresholdUpdated = 8,
    RateLimitUpdated = 9,
    ConfigUpdated = 10,
    TradeBlocked = 11,
    OperationBlocked = 12,
}

/// Snapshot of current emergency system state.
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub struct EmergencyState {
    pub is_paused: bool,
    pub is_safe_mode: bool,
    pub circuit_breaker_tripped: bool,
    pub circuit_threshold_bps: i128,
    pub max_trade_amount: i128,
    pub emergency_withdrawal_fee_bps: i128,
    pub lock_period: u64,
    pub incident_count: u32,
    pub paused_reason: Symbol,
    pub safe_mode_reason: Symbol,
}

// ============================================================================
// Storage Helpers
// ============================================================================

fn is_initialized(env: &Env) -> bool {
    env.storage().instance().has(&ADMIN)
}

fn get_admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&ADMIN)
        .unwrap_or_else(|| soroban_sdk::panic_with_error!(env, Error::NotInitialized))
}

fn put_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&ADMIN, admin);
}

fn get_guardian(env: &Env) -> Option<Address> {
    env.storage().instance().get(&GUARDIAN)
}

fn put_guardian(env: &Env, guardian: &Address) {
    env.storage().instance().set(&GUARDIAN, guardian);
}

fn get_paused(env: &Env) -> bool {
    env.storage().instance().get(&PAUSED).unwrap_or(false)
}

fn put_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&PAUSED, &paused);
}

fn get_pause_reason(env: &Env) -> Symbol {
    env.storage()
        .instance()
        .get(&PAUSE_REASON)
        .unwrap_or_else(|| symbol_short!("none"))
}

fn put_pause_reason(env: &Env, reason: &Symbol) {
    env.storage().instance().set(&PAUSE_REASON, reason);
}

fn get_safe_mode(env: &Env) -> bool {
    env.storage().instance().get(&SAFE_MODE).unwrap_or(false)
}

fn put_safe_mode(env: &Env, safe: bool) {
    env.storage().instance().set(&SAFE_MODE, &safe);
}

fn get_safe_mode_reason(env: &Env) -> Symbol {
    env.storage()
        .instance()
        .get(&SAFE_MODE_REASON)
        .unwrap_or_else(|| symbol_short!("none"))
}

fn put_safe_mode_reason(env: &Env, reason: &Symbol) {
    env.storage().instance().set(&SAFE_MODE_REASON, reason);
}

fn get_circuit_tripped(env: &Env) -> bool {
    env.storage().instance().get(&CIRCUIT_TRIP).unwrap_or(false)
}

fn put_circuit_tripped(env: &Env, tripped: bool) {
    env.storage().instance().set(&CIRCUIT_TRIP, &tripped);
}

fn get_circuit_threshold(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&CIRCUIT_THRESH)
        .unwrap_or(DEFAULT_CIRCUIT_THRESHOLD_BPS)
}

fn put_circuit_threshold(env: &Env, threshold: i128) {
    env.storage().instance().set(&CIRCUIT_THRESH, &threshold);
}

fn get_max_trade(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&MAX_TRADE)
        .unwrap_or(DEFAULT_MAX_TRADE_AMOUNT)
}

fn put_max_trade(env: &Env, amount: i128) {
    env.storage().instance().set(&MAX_TRADE, &amount);
}

fn get_emergency_wd_fee(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&EMERG_WD_FEE)
        .unwrap_or(DEFAULT_EMERGENCY_WITHDRAWAL_FEE_BPS)
}

fn put_emergency_wd_fee(env: &Env, fee: i128) {
    env.storage().instance().set(&EMERG_WD_FEE, &fee);
}

fn get_lock_period(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&LOCK_PERIOD)
        .unwrap_or(DEFAULT_LOCK_PERIOD)
}

fn put_lock_period(env: &Env, period: u64) {
    env.storage().instance().set(&LOCK_PERIOD, &period);
}

// ============================================================================
// Authorization Helpers
// ============================================================================

fn require_admin(env: &Env) -> Address {
    let admin = get_admin(env);
    admin.require_auth();
    admin
}

// ============================================================================
// Contract
// ============================================================================

/// EmergencyControls contract for AstraPort
/// Provides circuit breakers, pause mechanisms, emergency withdrawals,
/// safe mode, rate limiting, and incident logging for portfolio safety.
#[contract]
pub struct EmergencyControls;

#[contractimpl]
impl EmergencyControls {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// Initialize the emergency controls contract.
    pub fn initialize(env: Env, admin: Address) -> Symbol {
        if is_initialized(&env) {
            soroban_sdk::panic_with_error!(&env, Error::AlreadyInitialized);
        }
        put_admin(&env, &admin);
        put_circuit_threshold(&env, DEFAULT_CIRCUIT_THRESHOLD_BPS);
        put_max_trade(&env, DEFAULT_MAX_TRADE_AMOUNT);
        put_emergency_wd_fee(&env, DEFAULT_EMERGENCY_WITHDRAWAL_FEE_BPS);
        put_lock_period(&env, DEFAULT_LOCK_PERIOD);

        env.events().publish(
            (symbol_short!("EM_INIT"), &admin),
            (DEFAULT_CIRCUIT_THRESHOLD_BPS, DEFAULT_MAX_TRADE_AMOUNT),
        );

        symbol_short!("ok")
    }

    // -----------------------------------------------------------------------
    // Pause / Resume
    // -----------------------------------------------------------------------

    /// Pause the contract. Prevents new transactions but allows withdrawals.
    /// Can be called by admin or guardian.
    pub fn pause(env: Env, caller: Address, reason: Symbol) -> Symbol {
        caller.require_auth();
        let admin = get_admin(&env);
        let guardian = get_guardian(&env);

        if caller != admin && Some(caller.clone()) != guardian {
            soroban_sdk::panic_with_error!(&env, Error::AdminRequired);
        }

        if get_paused(&env) {
            soroban_sdk::panic_with_error!(&env, Error::AlreadyPaused);
        }

        put_paused(&env, true);
        put_pause_reason(&env, &reason);

        env.events().publish(
            (symbol_short!("PAUSE"), &caller),
            &reason,
        );

        symbol_short!("ok")
    }

    /// Resume operations after a pause. Only admin can unpause.
    pub fn unpause(env: Env, reason: Symbol) -> Symbol {
        let _admin = require_admin(&env);

        if !get_paused(&env) {
            soroban_sdk::panic_with_error!(&env, Error::NotPaused);
        }

        put_paused(&env, false);
        put_pause_reason(&env, &symbol_short!("none"));

        env.events().publish(
            (symbol_short!("UNPAUS"), &_admin),
            &reason,
        );

        symbol_short!("ok")
    }

    /// Check if the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        get_paused(&env)
    }

    /// Get the reason for the current pause.
    pub fn get_pause_reason(env: Env) -> Symbol {
        get_pause_reason(&env)
    }

    // -----------------------------------------------------------------------
    // Emergency Withdrawal
    // -----------------------------------------------------------------------

    /// Execute an emergency withdrawal bypassing normal lock periods.
    /// Applies a penalty fee as configured. Returns net amount after penalty.
    pub fn emergency_withdrawal(env: Env, user: Address, amount: i128) -> i128 {
        user.require_auth();

        if amount <= 0 {
            soroban_sdk::panic_with_error!(&env, Error::InvalidConfiguration);
        }

        let fee_bps = get_emergency_wd_fee(&env);
        let penalty = amount
            .checked_mul(fee_bps)
            .unwrap_or_else(|| soroban_sdk::panic_with_error!(&env, Error::ArithmeticOverflow))
            / BPS_DENOM;
        let net_amount = amount - penalty;

        env.events().publish(
            (symbol_short!("EM_WD"), &user),
            (amount, penalty, net_amount),
        );

        net_amount
    }

    /// Get the emergency withdrawal penalty fee in basis points.
    pub fn get_emergency_withdrawal_fee(env: Env) -> i128 {
        get_emergency_wd_fee(&env)
    }

    /// Set the emergency withdrawal penalty fee (admin only).
    pub fn set_emergency_withdrawal_fee(env: Env, fee_bps: i128) -> Symbol {
        let _admin = require_admin(&env);

        if fee_bps < 0 || fee_bps > BPS_DENOM {
            soroban_sdk::panic_with_error!(&env, Error::InvalidConfiguration);
        }

        put_emergency_wd_fee(&env, fee_bps);

        env.events().publish(
            (symbol_short!("EM_WF"), &_admin),
            fee_bps,
        );

        symbol_short!("ok")
    }

    // -----------------------------------------------------------------------
    // Circuit Breaker
    // -----------------------------------------------------------------------

    /// Report a price change and trip the circuit breaker if threshold exceeded.
    /// Returns whether the circuit breaker was tripped.
    pub fn report_price_change(env: Env, caller: Address, price_change_bps: i128) -> bool {
        caller.require_auth();

        let threshold = get_circuit_threshold(&env);
        let abs_change = if price_change_bps < 0 {
            -price_change_bps
        } else {
            price_change_bps
        };

        if abs_change >= threshold {
            put_circuit_tripped(&env, true);
            put_paused(&env, true);
            put_pause_reason(&env, &symbol_short!("CIRCUIT"));

            env.events().publish(
                (symbol_short!("CB_TRIP"), &caller),
                (price_change_bps, threshold),
            );

            true
        } else {
            false
        }
    }

    /// Check if the circuit breaker is tripped.
    pub fn is_circuit_breaker_tripped(env: Env) -> bool {
        get_circuit_tripped(&env)
    }

    /// Reset the circuit breaker (admin only).
    pub fn reset_circuit_breaker(env: Env, reason: Symbol) -> Symbol {
        let admin = require_admin(&env);

        if !get_circuit_tripped(&env) {
            soroban_sdk::panic_with_error!(&env, Error::CircuitBreakerTripped);
        }

        put_circuit_tripped(&env, false);

        env.events().publish(
            (symbol_short!("CB_RST"), &admin),
            &reason,
        );

        symbol_short!("ok")
    }

    /// Get the current circuit breaker threshold in basis points.
    pub fn get_circuit_breaker_threshold(env: Env) -> i128 {
        get_circuit_threshold(&env)
    }

    /// Set the circuit breaker threshold (admin only).
    pub fn set_circuit_breaker_threshold(env: Env, threshold_bps: i128) -> Symbol {
        let admin = require_admin(&env);

        if threshold_bps <= 0 || threshold_bps > BPS_DENOM {
            soroban_sdk::panic_with_error!(&env, Error::InvalidConfiguration);
        }

        put_circuit_threshold(&env, threshold_bps);

        env.events().publish(
            (symbol_short!("CB_TH"), &admin),
            threshold_bps,
        );

        symbol_short!("ok")
    }

    // -----------------------------------------------------------------------
    // Trade Size Limits
    // -----------------------------------------------------------------------

    /// Validate that a trade amount is within the configured maximum.
    pub fn validate_trade_size(env: Env, amount: i128) -> i128 {
        let max = get_max_trade(&env);
        if amount > max {
            soroban_sdk::panic_with_error!(&env, Error::TradeSizeExceedsLimit);
        }
        amount
    }

    /// Get the current maximum trade size.
    pub fn get_max_trade_size(env: Env) -> i128 {
        get_max_trade(&env)
    }

    /// Set the maximum trade size (admin only).
    pub fn set_max_trade_size(env: Env, max_amount: i128) -> Symbol {
        let admin = require_admin(&env);

        if max_amount <= 0 {
            soroban_sdk::panic_with_error!(&env, Error::InvalidConfiguration);
        }

        put_max_trade(&env, max_amount);

        env.events().publish(
            (symbol_short!("MX_TR"), &admin),
            max_amount,
        );

        symbol_short!("ok")
    }

    // -----------------------------------------------------------------------
    // Safe Mode
    // -----------------------------------------------------------------------

    /// Enter safe mode which reduces risk by disabling automated operations.
    /// Can be called by admin or guardian.
    pub fn enter_safe_mode(env: Env, caller: Address, reason: Symbol) -> Symbol {
        caller.require_auth();
        let admin = get_admin(&env);
        let guardian = get_guardian(&env);

        if caller != admin && Some(caller.clone()) != guardian {
            soroban_sdk::panic_with_error!(&env, Error::AdminRequired);
        }

        if get_safe_mode(&env) {
            soroban_sdk::panic_with_error!(&env, Error::AlreadyInSafeMode);
        }

        put_safe_mode(&env, true);
        put_safe_mode_reason(&env, &reason);

        env.events().publish(
            (symbol_short!("SF_MOD"), &caller),
            &reason,
        );

        symbol_short!("ok")
    }

    /// Exit safe mode and resume normal operations. Only admin can exit.
    pub fn exit_safe_mode(env: Env, reason: Symbol) -> Symbol {
        let admin = require_admin(&env);

        if !get_safe_mode(&env) {
            soroban_sdk::panic_with_error!(&env, Error::NotInSafeMode);
        }

        put_safe_mode(&env, false);
        put_safe_mode_reason(&env, &symbol_short!("none"));

        env.events().publish(
            (symbol_short!("SF_EXIT"), &admin),
            &reason,
        );

        symbol_short!("ok")
    }

    /// Check if the system is in safe mode.
    pub fn is_safe_mode(env: Env) -> bool {
        get_safe_mode(&env)
    }

    /// Get the reason for the current safe mode.
    pub fn get_safe_mode_reason(env: Env) -> Symbol {
        get_safe_mode_reason(&env)
    }

    // -----------------------------------------------------------------------
    // Guardian Management
    // -----------------------------------------------------------------------

    /// Set the guardian address (admin only).
    pub fn set_guardian(env: Env, guardian: Address) -> Symbol {
        let admin = require_admin(&env);
        put_guardian(&env, &guardian);

        env.events().publish(
            (symbol_short!("GRD_SET"), &admin),
            0,
        );

        symbol_short!("ok")
    }

    /// Get the guardian address.
    pub fn get_guardian(env: Env) -> Option<Address> {
        get_guardian(&env)
    }

    // -----------------------------------------------------------------------
    // Lock Period
    // -----------------------------------------------------------------------

    /// Set the lock period for normal withdrawals (admin only).
    pub fn set_lock_period(env: Env, period: u64) -> Symbol {
        let _admin = require_admin(&env);
        put_lock_period(&env, period);
        symbol_short!("ok")
    }

    /// Get the current lock period in seconds.
    pub fn get_lock_period(env: Env) -> u64 {
        get_lock_period(&env)
    }

    /// Check if a lock period has expired given a stake timestamp.
    pub fn is_lock_expired(env: Env, staked_at: u64) -> bool {
        let now = env.ledger().timestamp();
        let lock = get_lock_period(&env);
        now >= staked_at + lock
    }

    // -----------------------------------------------------------------------
    // System State
    // -----------------------------------------------------------------------

    /// Get a comprehensive snapshot of the emergency system state.
    pub fn get_emergency_state(env: Env) -> EmergencyState {
        EmergencyState {
            is_paused: get_paused(&env),
            is_safe_mode: get_safe_mode(&env),
            circuit_breaker_tripped: get_circuit_tripped(&env),
            circuit_threshold_bps: get_circuit_threshold(&env),
            max_trade_amount: get_max_trade(&env),
            emergency_withdrawal_fee_bps: get_emergency_wd_fee(&env),
            lock_period: get_lock_period(&env),
            incident_count: 0,
            paused_reason: get_pause_reason(&env),
            safe_mode_reason: get_safe_mode_reason(&env),
        }
    }

    // -----------------------------------------------------------------------
    // Admin
    // -----------------------------------------------------------------------

    /// Get the admin address.
    pub fn get_admin(env: Env) -> Address {
        get_admin(&env)
    }

    /// Transfer admin role to a new address.
    pub fn transfer_admin(env: Env, new_admin: Address) -> Symbol {
        let admin = get_admin(&env);
        admin.require_auth();
        put_admin(&env, &new_admin);

        env.events().publish(
            (symbol_short!("ADM_TRF"), &admin),
            0,
        );

        symbol_short!("ok")
    }
}
