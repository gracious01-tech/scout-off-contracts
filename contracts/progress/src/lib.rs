mod errors;
mod events;
mod types;

use errors::ProgressError;
use types::{DataKey, ProgressEntry, ProgressLevel};

use soroban_sdk::{contract, contractimpl, Address, Env};

const INSTANCE_TTL_MIN: u32 = 100;
const INSTANCE_TTL_MAX: u32 = 500;

#[contract]
pub struct ProgressContract;

#[contractimpl]
impl ProgressContract {
    // -------------------------------------------------------------------------
    // Admin
    // -------------------------------------------------------------------------

    pub fn initialize(env: Env, admin: Address) -> Result<(), ProgressError> {
        Self::bump_instance_ttl(&env);
        if env.storage().instance().has(&DataKey::Initialized) {
            return Err(ProgressError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Initialized, &true);
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    pub fn pause_contract(env: Env) -> Result<(), ProgressError> {
        Self::bump_instance_ttl(&env);
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        Ok(())
    }

    pub fn unpause_contract(env: Env) -> Result<(), ProgressError> {
        Self::bump_instance_ttl(&env);
        Self::require_admin(&env)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    /// Transfer admin rights to a new address (current admin auth required).
    pub fn transfer_admin(env: Env, new_admin: Address) -> Result<(), ProgressError> {
        Self::bump_instance_ttl(&env);
        let old_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ProgressError::NotInitialized)?;
        old_admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        events::admin_transferred(&env, &old_admin, &new_admin);
        Ok(())
    }

    /// Reset a player's level for dispute resolution.
    /// Existing history is preserved; a new history entry records the reset.
    pub fn reset_player_level(
        env: Env,
        player_id: u64,
        target_level: ProgressLevel,
    ) -> Result<(), ProgressError> {
        Self::require_not_paused(&env)?;
        let admin = Self::require_admin(&env)?;

        let old_level = Self::get_current_level(&env, player_id);
        Self::record_progress_entry(
            &env,
            player_id,
            old_level.clone(),
            target_level.clone(),
            admin,
            0,
        );
        env.storage()
            .persistent()
            .set(&DataKey::PlayerLevel(player_id), &target_level);

        events::player_level_reset(&env, player_id, &old_level, &target_level);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Progress updates
    // -------------------------------------------------------------------------

    /// Advance a player's progress level by one tier.
    /// Caller must be an authorized validator (or scout for Level 3).
    /// `milestone_ref` links back to the verification contract's milestone index.
    pub fn advance_level(
        env: Env,
        caller: Address,
        player_id: u64,
        milestone_ref: u32,
    ) -> Result<ProgressLevel, ProgressError> {
        Self::bump_instance_ttl(&env);
        Self::require_not_paused(&env)?;
        Self::require_initialized(&env)?;

        if let Some(verification_contract) = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::VerificationContract)
        {
            // When configured, only the verification contract may invoke this
            // function (directly or via cross-contract call). The `caller`
            // argument still records the validator or scout that triggered it.
            verification_contract.require_auth();
        } else {
            caller.require_auth();
        }

        let current = Self::get_current_level(&env, player_id);
        let new_level = current.next().ok_or(ProgressError::AlreadyAtMaxLevel)?;

        Self::record_progress_entry(
            &env,
            player_id,
            current,
            new_level.clone(),
            caller.clone(),
            milestone_ref,
        );
        env.storage()
            .persistent()
            .set(&DataKey::PlayerLevel(player_id), &new_level);

        events::progress_updated(&env, player_id, &new_level, &caller, milestone_ref);
        Ok(new_level)
    }

    // -------------------------------------------------------------------------
    // Queries
    // -------------------------------------------------------------------------

    pub fn get_level(env: Env, player_id: u64) -> Result<ProgressLevel, ProgressError> {
        env.storage()
            .persistent()
            .get(&DataKey::PlayerLevel(player_id))
            .ok_or(ProgressError::PlayerNotFound)
    }

    pub fn get_history_count(env: Env, player_id: u64) -> u32 {
        Self::bump_instance_ttl(&env);
        env.storage()
            .persistent()
            .get(&DataKey::HistoryCounter(player_id))
            .unwrap_or(0u32)
    }

    pub fn get_history_entry(
        env: Env,
        player_id: u64,
        index: u32,
    ) -> Result<ProgressEntry, ProgressError> {
        Self::bump_instance_ttl(&env);
        env.storage()
            .persistent()
            .get(&DataKey::HistoryEntry(player_id, index))
            .ok_or(ProgressError::PlayerNotFound)
    }

    pub fn health(env: Env) -> bool {
        Self::bump_instance_ttl(&env);
        env.storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::Initialized)
            .unwrap_or(false)
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    fn bump_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_MIN, INSTANCE_TTL_MAX);
    }

    fn get_current_level(env: &Env, player_id: u64) -> ProgressLevel {
        env.storage()
            .persistent()
            .get(&DataKey::PlayerLevel(player_id))
            .unwrap_or(ProgressLevel::Unverified)
    }

    fn record_progress_entry(
        env: &Env,
        player_id: u64,
        old_level: ProgressLevel,
        new_level: ProgressLevel,
        updated_by: Address,
        milestone_ref: u32,
    ) {
        let history_key = DataKey::HistoryCounter(player_id);
        let index: u32 = env.storage().persistent().get(&history_key).unwrap_or(0u32);
        let next_index = index.checked_add(1).expect("overflow");

        let entry = ProgressEntry {
            player_id,
            old_level,
            new_level,
            updated_by,
            updated_at: env.ledger().timestamp(),
            milestone_ref,
        };

        env.storage()
            .persistent()
            .set(&DataKey::HistoryEntry(player_id, next_index), &entry);
        env.storage().persistent().set(&history_key, &next_index);
    }

    fn require_initialized(env: &Env) -> Result<(), ProgressError> {
        if !env
            .storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::Initialized)
            .unwrap_or(false)
        {
            return Err(ProgressError::NotInitialized);
        }
        Ok(())
    }

    fn require_not_paused(env: &Env) -> Result<(), ProgressError> {
        if env
            .storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::Paused)
            .unwrap_or(false)
        {
            return Err(ProgressError::ContractPaused);
        }
        Ok(())
    }

    fn require_admin(env: &Env) -> Result<Address, ProgressError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ProgressError::NotInitialized)?;
        admin.require_auth();
        Ok(admin)
    }
}

// =============================================================================
// Tests
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Events as _},
        vec, Env, IntoVal, Symbol,
    };

    fn setup() -> (Env, ProgressContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register_contract(None, ProgressContract);
        let client = ProgressContractClient::new(&env, &id);
        (env, client)
    }

    #[test]
    fn test_two_players_advance_independently() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        let validator = Address::generate(&env);

        // Player 1: advance to Level 2 (PerformanceMilestones)
        client.advance_level(&validator, &1u64, &1u32);
        client.advance_level(&validator, &1u64, &2u32);

        // Player 2: advance to Level 1 (VerifiedIdentity)
        client.advance_level(&validator, &2u64, &3u32);

        assert_eq!(client.get_level(&1u64), ProgressLevel::PerformanceMilestones);
        assert_eq!(client.get_level(&2u64), ProgressLevel::VerifiedIdentity);
        assert_eq!(client.get_history_count(&1u64), 2);
        assert_eq!(client.get_history_count(&2u64), 1);
    }

    #[test]
    fn test_advance_level_sequence() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        let validator = Address::generate(&env);
        let player_id = 1u64;

        // Unverified → VerifiedIdentity
        let l1 = client.advance_level(&validator, &player_id, &1u32);
        assert_eq!(l1, ProgressLevel::VerifiedIdentity);

        // VerifiedIdentity → PerformanceMilestones
        let l2 = client.advance_level(&validator, &player_id, &2u32);
        assert_eq!(l2, ProgressLevel::PerformanceMilestones);

        // PerformanceMilestones → EliteTier
        let l3 = client.advance_level(&validator, &player_id, &3u32);
        assert_eq!(l3, ProgressLevel::EliteTier);

        assert_eq!(client.get_history_count(&player_id), 3);
    }

    #[test]
    #[should_panic]
    fn test_cannot_exceed_elite_tier() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        let validator = Address::generate(&env);
        let player_id = 1u64;

        client.advance_level(&validator, &player_id, &1u32);
        client.advance_level(&validator, &player_id, &2u32);
        client.advance_level(&validator, &player_id, &3u32);
        // This should panic — already at EliteTier
        client.advance_level(&validator, &player_id, &4u32);
    }

    #[test]
    fn test_transfer_admin_success() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        let new_admin = Address::generate(&env);
        // Should not panic — current admin auth is satisfied
        client.transfer_admin(&new_admin);
    }

    #[test]
    #[should_panic]
    fn test_transfer_admin_unauthorized() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        // Clear all mocks — no auth satisfied, so admin check fails
        env.mock_auths(&[]);
        client.transfer_admin(&Address::generate(&env));
    }

    #[test]
    #[should_panic]
    fn test_old_admin_loses_access_after_transfer() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        let new_admin = Address::generate(&env);
        client.transfer_admin(&new_admin);

        // Clear mocks — old admin auth no longer stored, so pause must fail
        env.mock_auths(&[]);
        client.pause_contract();
    }

    #[test]
    fn test_reset_player_level_success() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        let validator = Address::generate(&env);
        let player_id = 1u64;

        client.advance_level(&validator, &player_id, &1u32);
        client.advance_level(&validator, &player_id, &2u32);
        assert_eq!(client.get_history_count(&player_id), 2);

        client.reset_player_level(&player_id, &ProgressLevel::Unverified);

        assert_eq!(
            env.events().all(),
            vec![
                &env,
                (
                    client.address.clone(),
                    (Symbol::new(&env, "player_level_reset"),).into_val(&env),
                    (
                        player_id,
                        ProgressLevel::PerformanceMilestones,
                        ProgressLevel::Unverified,
                    )
                        .into_val(&env),
                ),
            ]
        );

        assert_eq!(client.get_level(&player_id), ProgressLevel::Unverified);
        assert_eq!(client.get_history_count(&player_id), 3);

        let reset_entry = client.get_history_entry(&player_id, &3u32);
        assert_eq!(reset_entry.old_level, ProgressLevel::PerformanceMilestones);
        assert_eq!(reset_entry.new_level, ProgressLevel::Unverified);
        assert_eq!(reset_entry.updated_by, admin);
        assert_eq!(reset_entry.milestone_ref, 0);
    }

    #[test]
    #[should_panic]
    fn test_reset_player_level_unauthorized() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        env.mock_auths(&[]);
        client.reset_player_level(&1u64, &ProgressLevel::Unverified);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #8)")]
    fn test_advance_level_history_counter_overflow() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        let caller = Address::generate(&env);
        let player_id = 1u64;

        env.as_contract(&client.address, || {
            env.storage()
                .persistent()
                .set(&DataKey::HistoryCounter(player_id), &u32::MAX);
        });

        client.advance_level(&caller, &player_id, &1u32);
    }
}
