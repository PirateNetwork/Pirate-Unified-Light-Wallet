use super::*;
use std::collections::{HashMap, HashSet};

/// Matches the historical desktop wallet's unused-address gap rule.
///
/// Shielded ownership cannot be queried without trial decryption, so discovery
/// must be bounded. Accounts 1 through 5 cover the legacy wallet's complete
/// default lookahead while keeping restore work deterministic.
pub(super) const LEGACY_SAPLING_ACCOUNT_GAP_LIMIT: u32 = 5;
const MAX_ZIP32_ACCOUNT_INDEX: u32 = 0x7fff_ffff;

const DISCOVERY_LOG_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct SeedAccountDiscoveryPreparation {
    pub candidates_added: usize,
    pub existing_keys_linked: usize,
    pub candidate_count: usize,
}

pub(super) fn mark_legacy_sapling_discovery_not_required(repo: &Repository<'_>) -> Result<()> {
    repo.set_legacy_sapling_account_discovery_complete(true)
        .map_err(|error| anyhow!(error.to_string()))
}

/// Add the next one or five ZIP-32 accounts as durable wallet keys.
///
/// Unlike restore-time lookahead candidates, these accounts are never retired
/// merely because a scan finds no notes. This makes repeated additions a
/// predictable way to reach sparse account layouts without accepting an
/// arbitrary account index from the UI.
pub(super) fn add_next_seed_accounts(wallet_id: &WalletId, count: u32) -> Result<Vec<u32>> {
    ensure_not_decoy("Add seed accounts")?;
    if count != 1 && count != 5 {
        return Err(anyhow!(
            "Seed accounts can only be added one or five at a time"
        ));
    }

    let wallet = get_wallet_meta(wallet_id)?;
    if wallet.watch_only {
        return Err(anyhow!(
            "Seed accounts are unavailable for view-only wallets"
        ));
    }

    let (_db, repo) = open_wallet_db_for(wallet_id)?;
    let secret = repo
        .get_wallet_secret(wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {wallet_id}"))?;
    let mnemonic_bytes = secret
        .encrypted_mnemonic
        .as_deref()
        .ok_or_else(|| anyhow!("This wallet does not have a seed phrase"))?;
    let mnemonic = std::str::from_utf8(mnemonic_bytes)
        .map_err(|_| anyhow!("Stored seed mnemonic is not valid UTF-8"))?;
    let mnemonic_language = wallet_secret_mnemonic_language(&secret, mnemonic)?;

    let metadata = repo.get_seed_derived_account_keys(secret.account_id)?;
    if metadata.iter().any(|entry| entry.is_discovery_candidate) {
        return Err(anyhow!(
            "Wait for the current seed account scan to finish before adding more accounts"
        ));
    }
    let previous_index = metadata.iter().map(|entry| entry.derivation_index).max();
    let first_index = previous_index
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| anyhow!("No more ZIP-32 account indices are available"))?;
    let last_index = first_index
        .checked_add(count - 1)
        .filter(|index| *index <= MAX_ZIP32_ACCOUNT_INDEX)
        .ok_or_else(|| anyhow!("No more ZIP-32 account indices are available"))?;

    let network = pirate_params::Network::from_type(wallet_network_type(wallet_id)?);
    let seed_bytes = ExtendedSpendingKey::seed_bytes_from_mnemonic_in_language(
        mnemonic,
        Some(mnemonic_language),
    )?;
    let ironwood_master = IronwoodExtendedSpendingKey::master(&seed_bytes)?;
    let existing_account_keys = repo.get_account_keys(secret.account_id)?;
    let created_at = chrono::Utc::now().timestamp();
    let mut accounts = Vec::with_capacity(count as usize);
    for (offset, derivation_index) in (first_index..=last_index).enumerate() {
        let sapling_extsk = ExtendedSpendingKey::from_mnemonic_with_account_and_language(
            mnemonic,
            network.network_type,
            derivation_index,
            Some(mnemonic_language),
        )?;
        let ironwood_extsk = ironwood_master.derive_account(network.coin_type, derivation_index)?;
        let sapling_extsk_bytes = sapling_extsk.to_bytes();
        let sapling_dfvk_bytes = sapling_extsk.to_extended_fvk().to_bytes();
        let ironwood_extsk_bytes = ironwood_extsk.to_bytes();
        let ironwood_fvk_bytes = ironwood_extsk.to_extended_fvk().to_bytes();
        let sapling_matches = |key: &AccountKey| {
            key.sapling_extsk.as_ref() == Some(&sapling_extsk_bytes)
                || (key.sapling_extsk.is_none()
                    && key.sapling_dfvk.as_ref() == Some(&sapling_dfvk_bytes))
        };
        let reusable_key = existing_account_keys
            .iter()
            .filter(|key| sapling_matches(key))
            .find(|key| {
                let ironwood_compatible = key
                    .orchard_extsk
                    .as_ref()
                    .is_none_or(|bytes| bytes == &ironwood_extsk_bytes)
                    && key
                        .orchard_fvk
                        .as_ref()
                        .is_none_or(|bytes| bytes == &ironwood_fvk_bytes);
                ironwood_compatible
            });
        if reusable_key.is_none() && existing_account_keys.iter().any(sapling_matches) {
            return Err(anyhow!(
                "An existing Sapling key for seed account {derivation_index} has incompatible Ironwood material"
            ));
        }
        let key = AccountKey {
            id: reusable_key.and_then(|key| key.id),
            account_id: secret.account_id,
            key_type: KeyType::ImportSpend,
            key_scope: KeyScope::Account,
            label: reusable_key
                .and_then(|key| key.label.clone())
                .or_else(|| Some(format!("Seed account {derivation_index}"))),
            birthday_height: reusable_key.map_or(i64::from(wallet.birthday_height), |key| {
                if key.birthday_height > 0 {
                    key.birthday_height.min(i64::from(wallet.birthday_height))
                } else {
                    i64::from(wallet.birthday_height)
                }
            }),
            created_at: reusable_key.map_or(created_at.saturating_add(offset as i64), |key| {
                key.created_at
            }),
            spendable: true,
            sapling_extsk: Some(sapling_extsk_bytes),
            sapling_dfvk: Some(sapling_dfvk_bytes),
            orchard_extsk: Some(ironwood_extsk_bytes),
            orchard_fvk: Some(ironwood_fvk_bytes),
            encrypted_mnemonic: None,
        };
        accounts.push((derivation_index, repo.encrypt_account_key_fields(&key)?));
    }

    repo.insert_seed_derived_accounts(secret.account_id, previous_index, &accounts)?;
    sync_control::clear_wallet_data_caches(wallet_id);
    let added = (first_index..=last_index).collect::<Vec<_>>();
    append_discovery_event(
        wallet_id,
        "user_added",
        serde_json::json!({
            "first_index": first_index,
            "last_index": last_index,
            "count": count,
            "birthday_height": wallet.birthday_height,
            "sapling": true,
            "ironwood": true,
        }),
    );
    Ok(added)
}

pub(super) fn prepare_legacy_sapling_account_discovery(
    repo: &Repository<'_>,
    secret: &WalletSecret,
    birthday_height: u32,
) -> Result<SeedAccountDiscoveryPreparation> {
    if secret.extsk.is_empty() || secret.encrypted_mnemonic.is_none() {
        return Ok(SeedAccountDiscoveryPreparation::default());
    }
    if repo.legacy_sapling_account_discovery_complete()? {
        return Ok(SeedAccountDiscoveryPreparation::default());
    }

    let mnemonic_bytes = secret
        .encrypted_mnemonic
        .as_deref()
        .ok_or_else(|| anyhow!("Seed mnemonic is unavailable for account discovery"))?;
    let mnemonic = std::str::from_utf8(mnemonic_bytes)
        .map_err(|_| anyhow!("Stored seed mnemonic is not valid UTF-8"))?;
    let mnemonic_language = wallet_secret_mnemonic_language(secret, mnemonic)?;
    let network = pirate_params::Network::mainnet();

    let account_keys = repo.get_account_keys(secret.account_id)?;
    let existing_by_extsk = account_keys
        .iter()
        .filter_map(|key| {
            key.sapling_extsk
                .as_ref()
                .map(|extsk| (extsk.clone(), key.id))
        })
        .filter_map(|(extsk, id)| id.map(|id| (extsk, id)))
        .collect::<HashMap<_, _>>();
    let existing_metadata = repo.get_seed_derived_account_keys(secret.account_id)?;
    let existing_indices = existing_metadata
        .iter()
        .map(|metadata| metadata.derivation_index)
        .collect::<HashSet<_>>();

    let mut outcome = SeedAccountDiscoveryPreparation::default();
    for derivation_index in 1..=LEGACY_SAPLING_ACCOUNT_GAP_LIMIT {
        if existing_indices.contains(&derivation_index) {
            continue;
        }

        let extsk = ExtendedSpendingKey::from_mnemonic_with_account_and_language(
            mnemonic,
            network.network_type,
            derivation_index,
            Some(mnemonic_language),
        )?;
        let extsk_bytes = extsk.to_bytes();
        let key_id = if let Some(key_id) = existing_by_extsk.get(&extsk_bytes).copied() {
            // Never retire a key the user explicitly imported before discovery.
            repo.upsert_seed_derived_account_key(
                key_id,
                secret.account_id,
                derivation_index,
                false,
            )?;
            outcome.existing_keys_linked += 1;
            key_id
        } else {
            let key = AccountKey {
                id: None,
                account_id: secret.account_id,
                key_type: KeyType::ImportSpend,
                key_scope: KeyScope::Account,
                label: Some(format!("Seed account {derivation_index}")),
                birthday_height: i64::from(birthday_height),
                created_at: chrono::Utc::now().timestamp(),
                spendable: true,
                sapling_extsk: Some(extsk_bytes),
                sapling_dfvk: Some(extsk.to_extended_fvk().to_bytes()),
                orchard_extsk: None,
                orchard_fvk: None,
                encrypted_mnemonic: None,
            };
            let encrypted = repo.encrypt_account_key_fields(&key)?;
            let key_id = repo.upsert_account_key(&encrypted)?;
            repo.upsert_seed_derived_account_key(
                key_id,
                secret.account_id,
                derivation_index,
                true,
            )?;
            outcome.candidates_added += 1;
            key_id
        };

        tracing::debug!(
            key_id,
            derivation_index,
            "Prepared legacy Sapling seed account lookahead"
        );
    }

    outcome.candidate_count = repo
        .get_seed_derived_account_keys(secret.account_id)?
        .into_iter()
        .filter(|metadata| metadata.is_discovery_candidate)
        .count();
    append_discovery_event(
        &secret.wallet_id,
        "prepared",
        serde_json::json!({
            "candidates_added": outcome.candidates_added,
            "existing_keys_linked": outcome.existing_keys_linked,
            "candidate_count": outcome.candidate_count,
            "gap_limit": LEGACY_SAPLING_ACCOUNT_GAP_LIMIT,
        }),
    );
    Ok(outcome)
}

pub(super) fn finalize_legacy_sapling_account_discovery(
    wallet_id: &WalletId,
) -> Result<pirate_storage_sqlite::SeedAccountDiscoveryFinalization> {
    let (_db, repo) = open_wallet_db_for(wallet_id)?;
    if repo.legacy_sapling_account_discovery_complete()? {
        return Ok(pirate_storage_sqlite::SeedAccountDiscoveryFinalization::default());
    }
    let secret = repo
        .get_wallet_secret(wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {wallet_id}"))?;
    let outcome = repo.finalize_legacy_sapling_account_discovery(secret.account_id)?;
    if repo.legacy_sapling_account_discovery_complete()? {
        append_discovery_event(
            wallet_id,
            "finalized",
            serde_json::json!({
                "retained": outcome.retained,
                "retired": outcome.retired,
                "highest_used_index": outcome.highest_used_index,
                "gap_limit": LEGACY_SAPLING_ACCOUNT_GAP_LIMIT,
            }),
        );
    }
    Ok(outcome)
}

fn append_discovery_event(wallet_id: &str, phase: &str, data: serde_json::Value) {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let event = serde_json::json!({
        "id": "log_seed_account_discovery",
        "timestamp": timestamp,
        "location": "api::seed_account_discovery",
        "message": "seed account discovery",
        "data": {
            "schema_version": DISCOVERY_LOG_SCHEMA_VERSION,
            "wallet_id": wallet_id,
            "phase": phase,
            "details": data,
        },
        "sessionId": "debug-session",
        "runId": "run1",
        "hypothesisId": "K",
    });
    pirate_core::debug_log::append_line(&event.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use pirate_storage_sqlite::{EncryptionAlgorithm, MasterKey};

    const MNEMONIC: &str =
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[test]
    fn preparation_derives_the_legacy_gap_once_and_finalization_retires_misses() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let key = EncryptionKey::from_passphrase("legacy-discovery", &[0x44; 32]).unwrap();
        let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
        let db = Database::open(file.path(), &key, master_key).unwrap();
        let repo = Repository::new(&db);
        let account_id = repo
            .insert_account(&Account {
                id: None,
                name: "Restored".to_string(),
                created_at: 1,
            })
            .unwrap();
        let account_zero = ExtendedSpendingKey::from_mnemonic_with_account_and_language(
            MNEMONIC,
            NetworkType::Mainnet,
            0,
            Some(MnemonicLanguage::English),
        )
        .unwrap();
        let secret = WalletSecret {
            wallet_id: "restored-wallet".to_string(),
            account_id,
            extsk: account_zero.to_bytes(),
            dfvk: Some(account_zero.to_extended_fvk().to_bytes()),
            orchard_extsk: None,
            sapling_ivk: None,
            orchard_ivk: None,
            encrypted_mnemonic: Some(MNEMONIC.as_bytes().to_vec()),
            mnemonic_language: Some(MnemonicLanguage::English.as_key().to_string()),
            created_at: 1,
        };

        let first = prepare_legacy_sapling_account_discovery(&repo, &secret, 1).unwrap();
        let second = prepare_legacy_sapling_account_discovery(&repo, &secret, 1).unwrap();
        let metadata = repo.get_seed_derived_account_keys(account_id).unwrap();

        assert_eq!(first.candidates_added, 5);
        assert_eq!(first.candidate_count, 5);
        assert_eq!(second.candidates_added, 0);
        assert_eq!(second.candidate_count, 5);
        assert_eq!(
            metadata
                .iter()
                .map(|entry| entry.derivation_index)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
        for entry in &metadata {
            let stored = repo
                .get_account_key_by_id(entry.key_id)
                .unwrap()
                .expect("candidate account key");
            let expected = ExtendedSpendingKey::from_mnemonic_with_account_and_language(
                MNEMONIC,
                NetworkType::Mainnet,
                entry.derivation_index,
                Some(MnemonicLanguage::English),
            )
            .unwrap();
            assert_eq!(stored.sapling_extsk, Some(expected.to_bytes()));
        }

        let finalized = repo
            .finalize_legacy_sapling_account_discovery(account_id)
            .unwrap();
        assert_eq!(finalized.retired, 5);
        assert!(repo
            .get_seed_derived_account_keys(account_id)
            .unwrap()
            .is_empty());
    }
}
