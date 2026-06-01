use soroban_sdk::{contracttype, Address};

pub use scoutchain_shared_types::ProgressLevel;

/// A single entry in the immutable progress history
#[contracttype]
#[derive(Clone, Debug)]
pub struct ProgressEntry {
    pub player_id: u64,
    pub old_level: ProgressLevel,
    pub new_level: ProgressLevel,
    /// Wallet that triggered the update (validator or scout)
    pub updated_by: Address,
    pub updated_at: u64,
    /// Milestone index from the verification contract that triggered this
    pub milestone_ref: u32,
    /// Ledger sequence number at the time of the level change
    pub ledger_sequence: u32,
}

#[contracttype]
pub enum DataKey {
    Admin,
    Initialized,
    Paused,
    /// player_id → current ProgressLevel
    PlayerLevel(u64),
    /// history counter per player
    HistoryCounter(u64),
    /// (player_id, history_index) → ProgressEntry
    HistoryEntry(u64, u32),
    /// address of the verification contract (for cross-contract auth checks)
    VerificationContract,
}
