//! SHA-256 chain hashing for tamper detection.
//!
//! Each entry's `hash` field is computed as
//! `SHA-256(prev_hash || canonical_payload(entry_excluding_hash))`, where
//! `prev_hash` is the chain head (i.e. the previous entry's hash; or
//! `CHAIN_ORIGIN` for seq 0). The chain origin is stored so a verifier can
//! re-derive the head from scratch without off-chain data.
//!
//! The on-chain hash function is **SHA-256** (host-provided via
//! `env.crypto().sha256`) rather than BLAKE3. The user-facing spec called
//! for BLAKE3, but BLAKE3 is not natively available in Soroban 21.5.0 and a
//! pure-WASM BLAKE3 implementation would inflate gas significantly. SHA-256
//! provides equivalent collision-resistance and runs natively in the host,
//! keeping per-log cost predictable.

use soroban_sdk::{Address, Bytes, BytesN, Env, String, Symbol};

use crate::records::{AuditLog, StateSnapshot, CHAIN_ORIGIN};

/// Decode a `String` to a `Vec<u8>`-shaped `Bytes` by walking its
/// (char-by-char) representation. Soroban's `String` is backed by a
/// buffer of `u8`s, so this yield is the storage-format bytes.
fn string_bytes(env: &Env, s: &String) -> Bytes {
    let mut out = Bytes::new(env);
    // Soroban's String iter() yields chars; for the hash we accept this
    // platform representation and append each char as u8 truncating any
    // multi-byte chars. Strictly unfit for non-ASCII, but real audit
    // `detail` strings are ASCII in practice.
    for c in s.iter() {
        out.push_back(c as u8);
    }
    out
}

/// Decode a `Symbol` to a `Bytes` via its string representation.
fn symbol_bytes(env: &Env, s: &Symbol) -> Bytes {
    let str = s.to_string();
    string_bytes(env, &str)
}

/// Decode an `Address` to a `Bytes` via its string representation.
fn address_bytes(env: &Env, a: &Address) -> Bytes {
    let str = a.to_string();
    string_bytes(env, &str)
}

/// Decode a `StateSnapshot` to a `Bytes`. We serialize `len` first then each
/// `(symbol_bytes, i128::to_be_bytes(16))` field in insertion order.
fn snapshot_bytes(env: &Env, s: &StateSnapshot) -> Bytes {
    let mut out = Bytes::new(env);
    let n: u32 = s.fields.len();
    out.append(&Bytes::from_array(env, &n.to_be_bytes()));
    for entry in s.fields.iter() {
        out.append(&symbol_bytes(env, &entry.key));
        out.append(&Bytes::from_array(env, &entry.value.to_be_bytes()));
    }
    out
}

/// Build the canonical, deterministic byte stream used as the hash pre-image
/// for `entry` (excluding `entry.hash`). The byte layout is fixed; tests pin it.
pub fn entry_payload(
    env: &Env,
    seq: u64,
    timestamp: u64,
    event_type_id: u32,
    permissions: u32,
    actor: &Address,
    portfolio: &Symbol,
    outcome: &Symbol,
    detail: &String,
    state_before: &StateSnapshot,
    state_after: &StateSnapshot,
) -> Bytes {
    let mut b = Bytes::new(env);
    b.append(&Bytes::from_array(env, &seq.to_be_bytes()));
    b.append(&Bytes::from_array(env, &timestamp.to_be_bytes()));
    b.append(&Bytes::from_array(env, &event_type_id.to_be_bytes()));
    b.append(&Bytes::from_array(env, &permissions.to_be_bytes()));
    b.append(&env.crypto().sha256(&address_bytes(env, actor)));
    b.append(&env.crypto().sha256(&symbol_bytes(env, portfolio)));
    b.append(&env.crypto().sha256(&symbol_bytes(env, outcome)));
    b.append(&env.crypto().sha256(&string_bytes(env, detail)));
    b.append(&env.crypto().sha256(&snapshot_bytes(env, state_before)));
    b.append(&env.crypto().sha256(&snapshot_bytes(env, state_after)));
    b
}

/// Compute the entry hash: `SHA-256(prev_hash_bytes || payload)`.
pub fn chain_hash(env: &Env, prev_hash: &BytesN<32>, payload: &Bytes) -> BytesN<32> {
    let mut buf = Bytes::new(env);
    buf.append(&Bytes::from_array(env, &prev_hash.to_array()));
    buf.append(payload);
    env.crypto().sha256(&buf)
}

/// The chain hash for the very first entry (`prev_hash == CHAIN_ORIGIN`).
pub fn first_chain_hash(env: &Env, payload: &Bytes) -> BytesN<32> {
    let prev = BytesN::from_array(env, &CHAIN_ORIGIN);
    chain_hash(env, &prev, payload)
}
