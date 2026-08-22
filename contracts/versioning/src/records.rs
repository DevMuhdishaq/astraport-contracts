//! Record types (storage-friendly) for the versioning contract.
//!
//! Contains version metadata, upgrade proposal/approval records, migration
//! definitions, feature flags, and storage key enums.

use soroban_sdk::{contracttype, Address, Symbol, Vec};

// ---------------------------------------------------------------------------
// Version status
// ---------------------------------------------------------------------------

/// Lifecycle status of a deployed contract version.
#[contracttype]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VersionStatus {
    /// Version has been proposed but not yet activated.
    Proposed = 0,
    /// Version is the current live version.
    Active = 1,
    /// Version has been superseded by a newer version.
    Superseded = 2,
    /// Version has been frozen for archival; can never be upgraded from.
    Frozen = 3,
    /// Version was rolled back from an active state.
    RolledBack = 4,
}

// ---------------------------------------------------------------------------
// Version metadata
// ---------------------------------------------------------------------------

/// Metadata describing a single contract version.
#[contracttype]
#[derive(Debug, Clone)]
pub struct VersionMetadata {
    /// Monotonically increasing version number (starting at 1).
    pub version_number: u32,
    /// Human-readable semantic version string (e.g. "1.2.0").
    pub semantic_version: Symbol,
    /// Status of this version.
    pub status: VersionStatus,
    /// Address that proposed this version.
    pub proposer: Address,
    /// Ledger timestamp when this version was proposed.
    pub proposed_at: u64,
    /// Ledger timestamp when this version became active (0 if not yet active).
    pub activated_at: u64,
    /// Hash of the deployed WASM binary for this version.
    pub wasm_hash: soroban_sdk::BytesN<32>,
    /// Ordered list of migration step descriptions for upgrading to this version.
    pub migration_steps: Vec<Symbol>,
    /// Free-form description of changes in this version.
    pub description: Symbol,
}

// ---------------------------------------------------------------------------
// Upgrade proposal & multi-sig
// ---------------------------------------------------------------------------

/// Represents a single upgrade proposal pending multi-sig approval.
#[contracttype]
#[derive(Debug, Clone)]
pub struct UpgradeProposal {
    /// Unique proposal id.
    pub proposal_id: u64,
    /// The target version number to upgrade to.
    pub target_version: u32,
    /// Address of the admin who created this proposal.
    pub proposer: Address,
    /// Ledger timestamp when the proposal was created.
    pub created_at: u64,
    /// Addresses that have approved this proposal.
    pub approvals: Vec<Address>,
    /// Whether the proposal has been executed.
    pub executed: bool,
    /// Whether the proposal was rejected.
    pub rejected: bool,
}

// ---------------------------------------------------------------------------
// Feature flags
// ---------------------------------------------------------------------------

/// Status of a feature flag.
#[contracttype]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FeatureFlagStatus {
    /// Flag is disabled; feature is not available.
    Disabled = 0,
    /// Flag is enabled for all users.
    Enabled = 1,
    /// Flag is in gradual rollout mode (percentage-based).
    GradualRollout = 2,
}

/// A feature flag that controls availability of specific functionality.
#[contracttype]
#[derive(Debug, Clone)]
pub struct FeatureFlag {
    /// Unique flag name.
    pub flag_name: Symbol,
    /// Current status of the flag.
    pub status: FeatureFlagStatus,
    /// Rollout percentage (0–100) when status is `GradualRollout`.
    pub rollout_percentage: u32,
    /// Minimum version required for this feature to be available.
    pub min_version: u32,
    /// Description of what the flag controls.
    pub description: Symbol,
    /// Ledger timestamp when the flag was last modified.
    pub last_modified: u64,
}

// ---------------------------------------------------------------------------
// Migration record
// ---------------------------------------------------------------------------

/// Record of a completed migration between versions.
#[contracttype]
#[derive(Debug, Clone)]
pub struct MigrationRecord {
    /// The version that was migrated from.
    pub from_version: u32,
    /// The version that was migrated to.
    pub to_version: u32,
    /// Address that performed the migration.
    pub migrator: Address,
    /// Ledger timestamp when the migration completed.
    pub timestamp: u64,
    /// Whether the migration completed successfully.
    pub success: bool,
    /// Number of data items migrated.
    pub items_migrated: u64,
}

// ---------------------------------------------------------------------------
// Audit trail entry
// ---------------------------------------------------------------------------

/// A single entry in the version audit trail.
#[contracttype]
#[derive(Debug, Clone)]
pub struct VersionAuditEntry {
    /// Monotonically increasing sequence id.
    pub seq: u64,
    /// Ledger timestamp.
    pub timestamp: u64,
    /// What action was taken (e.g. "propose", "approve", "upgrade", "rollback").
    pub action: Symbol,
    /// Version number this action pertains to.
    pub version_number: u32,
    /// Actor who performed the action.
    pub actor: Address,
    /// Additional detail (free-form).
    pub detail: Symbol,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

/// Storage keys for the versioning contract.
#[contracttype]
#[derive(Debug, Clone)]
pub enum VersionStorageKey {
    /// Admin address set during `initialize`.
    Admin,
    /// Number of signers required to approve an upgrade (multi-sig threshold).
    ApprovalThreshold,
    /// Registered signer addresses (multi-sig participants).
    Signers,
    /// Current active version number.
    CurrentVersion,
    /// Metadata for a version, keyed by version number.
    VersionMetadata(u32),
    /// List of all version numbers that have been registered.
    AllVersions,
    /// Upgrade proposal keyed by proposal id.
    Proposal(u64),
    /// Next proposal id to allocate.
    NextProposalId,
    /// Feature flag keyed by flag name.
    FeatureFlag(Symbol),
    /// All registered feature flag names.
    AllFeatureFlags,
    /// Migration record keyed by (from_version, to_version).
    MigrationRecord(u32, u32),
    /// Version audit trail entries.
    AuditTrail,
    /// Next audit trail sequence id.
    NextAuditSeq,
    /// Frozen version numbers.
    FrozenVersions,
    /// Audit-log sink address (cross-contract integration).
    AuditSink,
}
