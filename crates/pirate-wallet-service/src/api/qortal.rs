use super::*;
use crate::models::{QortalSyncStatus, QortalTransaction, QortalTxMetadata};
use pirate_storage_sqlite::AddressScope;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Standard Qortal shielded send request.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QortalSendRequest {
    /// Wallet-owned shielded address whose notes may be selected.
    pub input: String,
    /// Shielded recipients.
    pub output: Vec<Output>,
    /// Fee in arrrtoshis.
    pub fee: Option<u64>,
}

#[derive(Debug, Default)]
struct SyncSession {
    sync_id: u64,
    active: bool,
    start_height: u64,
    target_height: u64,
}

lazy_static::lazy_static! {
    static ref SYNC_SESSIONS: parking_lot::Mutex<HashMap<WalletId, SyncSession>> =
        parking_lot::Mutex::new(HashMap::new());
}

/// Convert unified-core progress into the schema consumed by Qortal Core.
pub fn qortal_sync_status(wallet_id: WalletId) -> Result<QortalSyncStatus> {
    let mut status = sync_status(wallet_id.clone())?;
    if status.local_height == 0 {
        status.local_height = u64::from(get_wallet_meta(&wallet_id)?.birthday_height);
    }
    let mut sessions = SYNC_SESSIONS.lock();
    let session = sessions.entry(wallet_id).or_default();
    Ok(update_sync_session(session, &status))
}

fn update_sync_session(session: &mut SyncSession, status: &SyncStatus) -> QortalSyncStatus {
    let in_progress = status.is_syncing();

    if !in_progress {
        session.active = false;
        return QortalSyncStatus {
            sync_id: session.sync_id,
            in_progress: false,
            last_error: None,
            start_block: None,
            end_block: None,
            synced_blocks: None,
            trial_decryptions_blocks: None,
            txn_scan_blocks: None,
            total_blocks: None,
            batch_num: None,
            batch_total: None,
            scanned_height: Some(status.local_height),
        };
    }

    if !session.active {
        session.sync_id = session.sync_id.saturating_add(1);
        session.start_height = status.local_height;
        session.active = true;
    }
    session.target_height = session.target_height.max(status.target_height);

    let completed = status.local_height.saturating_sub(session.start_height);
    let total = session.target_height.saturating_sub(session.start_height);
    QortalSyncStatus {
        sync_id: session.sync_id,
        in_progress: true,
        last_error: None,
        start_block: Some(session.start_height),
        end_block: Some(session.target_height),
        synced_blocks: Some(completed),
        // The unified scanner decrypts and records transactions in one pipeline.
        // Report the same completed range for both legacy progress counters.
        trial_decryptions_blocks: Some(completed),
        txn_scan_blocks: Some(completed),
        total_blocks: Some(total),
        batch_num: Some(0),
        batch_total: Some(1),
        scanned_height: None,
    }
}

/// Return the legacy Qortal balance object with numeric arrrtoshi values.
pub fn qortal_balance(wallet_id: WalletId) -> Result<Value> {
    let balance = get_balance(wallet_id.clone())?;
    let mut address_balances = list_address_balances(wallet_id, None)?;
    address_balances.sort_by_key(|entry| {
        let pool_order = if entry.address.starts_with("zs")
            || entry.address.starts_with("ztestsapling")
            || entry.address.starts_with("zregtestsapling")
        {
            0
        } else {
            1
        };
        (pool_order, entry.diversifier_index, entry.created_at)
    });
    let z_addresses = address_balances
        .into_iter()
        .map(|entry| {
            json!({
                "address": entry.address,
                "zbalance": entry.balance,
                "verified_zbalance": entry.spendable,
                "spendable_zbalance": entry.spendable,
                "unverified_zbalance": entry.pending,
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "zbalance": balance.total,
        "verified_zbalance": balance.spendable,
        "spendable_zbalance": balance.spendable,
        "unverified_zbalance": balance.pending,
        "tbalance": 0,
        "z_addresses": z_addresses,
        "t_addresses": [],
    }))
}

/// Send using notes owned by the exact source address supplied by Qortal.
pub async fn qortal_send(wallet_id: WalletId, request: QortalSendRequest) -> Result<String> {
    let address_id = {
        let (_db, repo) = open_wallet_db_for(&wallet_id)?;
        let secret = repo
            .get_wallet_secret(&wallet_id)?
            .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;
        let source = repo
            .get_address_by_string(secret.account_id, &request.input)?
            .ok_or_else(|| {
                anyhow!(
                    "Input address {} is not owned by wallet {}",
                    request.input,
                    wallet_id
                )
            })?;
        source
            .id
            .ok_or_else(|| anyhow!("Input address row id is missing"))?
    };

    let address_filter = Some(vec![address_id]);
    let pending = build_tx_filtered(
        wallet_id.clone(),
        request.output,
        request.fee,
        None,
        address_filter.clone(),
    )?;
    let signed = sign_tx_filtered(wallet_id, pending, None, address_filter)?;
    tx_flow::broadcast_tx(signed).await
}

struct LocalQortalTransaction {
    transaction: QortalTransaction,
    should_recover_outgoing: bool,
}

fn load_qortal_transactions(
    wallet_id: &WalletId,
    limit: Option<u32>,
) -> Result<(Vec<LocalQortalTransaction>, HashMap<String, AddressScope>)> {
    if is_decoy_mode_active() {
        return Ok((Vec::new(), HashMap::new()));
    }

    let (db, repo) = open_wallet_db_for(wallet_id)?;
    let secret = repo
        .get_wallet_secret(wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;
    let sync_state = pirate_storage_sqlite::SyncStateStorage::new(&db).load_sync_state()?;
    let current_height = sync_state.local_height.max(sync_state.target_height);

    let addresses = repo.get_all_addresses(secret.account_id)?;
    let addresses_by_id = addresses
        .iter()
        .filter_map(|address| address.id.map(|id| (id, address.clone())))
        .collect::<HashMap<_, _>>();
    let scopes_by_address = addresses
        .into_iter()
        .map(|address| (address.address, address.address_scope))
        .collect::<HashMap<_, _>>();

    // Qortal expects exactly one entry per txid and separates change through
    // metadata arrays, so disable the GUI's split-transfer presentation.
    let records =
        repo.get_transactions_with_options(secret.account_id, limit, current_height, 1, false)?;
    let mut transactions = Vec::with_capacity(records.len());

    for record in records {
        let txid_bytes = hex::decode(&record.txid)
            .map_err(|err| anyhow!("Invalid stored transaction id: {}", err))?;
        let mut reversed_txid = txid_bytes.clone();
        reversed_txid.reverse();
        let mut notes = repo.get_notes_by_txid(secret.account_id, &txid_bytes)?;
        if notes.is_empty() {
            notes = repo.get_notes_by_txid(secret.account_id, &reversed_txid)?;
        }

        let mut incoming_metadata = Vec::new();
        let mut incoming_metadata_change = Vec::new();
        for note in notes {
            let Ok(value) = u64::try_from(note.value) else {
                continue;
            };
            if value == 0 {
                continue;
            }
            let address = note
                .address_id
                .and_then(|id| addresses_by_id.get(&id))
                .cloned();
            let metadata = QortalTxMetadata {
                address: address
                    .as_ref()
                    .map(|entry| entry.address.clone())
                    .unwrap_or_else(|| "[UNKNOWN]".to_string()),
                value,
                memo: note
                    .memo
                    .as_ref()
                    .and_then(|memo| pirate_sync_lightd::sapling::full_decrypt::decode_memo(memo)),
            };
            if address
                .as_ref()
                .is_some_and(|entry| entry.address_scope == AddressScope::Internal)
            {
                incoming_metadata_change.push(metadata);
            } else {
                incoming_metadata.push(metadata);
            }
        }

        let confirmed = record.height > 0 && current_height >= record.height as u64;
        let block_height = u32::try_from(record.height.max(0)).unwrap_or(u32::MAX);
        transactions.push(LocalQortalTransaction {
            should_recover_outgoing: record.amount < 0,
            transaction: QortalTransaction {
                block_height,
                datetime: record.timestamp,
                txid: record.txid,
                amount: record.amount,
                fee: record.fee,
                incoming_metadata,
                incoming_metadata_change,
                outgoing_metadata: Vec::new(),
                outgoing_metadata_change: Vec::new(),
                unconfirmed: (!confirmed).then_some(true),
            },
        });
    }

    Ok((transactions, scopes_by_address))
}

async fn recover_qortal_recipients(
    client: &LightClient,
    wallet_id: &WalletId,
    txid: &str,
) -> Result<Vec<TransactionRecipient>> {
    let (_endpoint_config, tx_hash_candidates, sapling_ovks, orchard_ovks, tx_height_hint) =
        collect_tx_recovery_context(wallet_id, txid)?;

    let mut last_error = None;
    for tx_hash in tx_hash_candidates {
        match client.get_transaction(&tx_hash).await {
            Ok(raw) => {
                return Ok(
                    payment_disclosure::recover_outgoing_recipients_with_disclosures_from_raw_tx(
                        &raw,
                        tx_height_hint,
                        &sapling_ovks,
                        &orchard_ovks,
                        address_prefix_network_type(wallet_id)?,
                    ),
                );
            }
            Err(err) => last_error = Some(err.to_string()),
        }
    }

    Err(anyhow!(
        "Failed to fetch transaction {}: {}",
        txid,
        last_error.unwrap_or_else(|| "not found".to_string())
    ))
}

/// Return transaction history with the metadata arrays Qortal actually reads.
pub async fn qortal_list_transactions(
    wallet_id: WalletId,
    limit: Option<u32>,
) -> Result<Vec<QortalTransaction>> {
    let (mut transactions, scopes_by_address) = load_qortal_transactions(&wallet_id, limit)?;
    let needs_outgoing_recovery = transactions
        .iter()
        .any(|entry| entry.should_recover_outgoing);
    let recovery_client = if needs_outgoing_recovery {
        let endpoint_config = get_lightd_endpoint_config(wallet_id.clone())?;
        let client_config = tunnel::light_client_config_for_endpoint(
            &endpoint_config,
            RetryConfig::default(),
            Duration::from_secs(30),
            Duration::from_secs(60),
        );
        let client = LightClient::with_config(client_config);
        match client.connect().await {
            Ok(()) => Some(client),
            Err(err) => {
                tracing::warn!(
                    "Could not connect for Qortal transaction metadata recovery: {}",
                    err
                );
                None
            }
        }
    } else {
        None
    };

    for entry in &mut transactions {
        if !entry.should_recover_outgoing {
            continue;
        }
        let recovered = match recovery_client.as_ref() {
            Some(client) => {
                recover_qortal_recipients(client, &wallet_id, &entry.transaction.txid).await
            }
            None => Err(anyhow!("lightwalletd is unavailable")),
        };
        match recovered {
            Ok(recipients) => {
                for recipient in recipients {
                    let metadata = QortalTxMetadata {
                        address: recipient.address.clone(),
                        value: recipient.amount,
                        memo: recipient.memo,
                    };
                    if scopes_by_address.get(&recipient.address) == Some(&AddressScope::Internal) {
                        entry.transaction.outgoing_metadata_change.push(metadata);
                    } else {
                        entry.transaction.outgoing_metadata.push(metadata);
                    }
                }
            }
            Err(err) => {
                tracing::warn!(
                    "Could not recover Qortal metadata for transaction {}: {}",
                    entry.transaction.txid,
                    err
                );
            }
        }

        if entry.transaction.outgoing_metadata.is_empty() {
            let external_value = entry
                .transaction
                .amount
                .unsigned_abs()
                .saturating_sub(entry.transaction.fee);
            if external_value > 0 {
                // Qortal Core calculates transaction values from metadata and
                // ignores the top-level amount. Preserve the correct value when
                // a historical raw transaction is temporarily unavailable.
                entry.transaction.outgoing_metadata.push(QortalTxMetadata {
                    address: "[UNKNOWN]".to_string(),
                    value: external_value,
                    memo: None,
                });
            }
        }
    }

    Ok(transactions
        .into_iter()
        .map(|entry| entry.transaction)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qortal_transaction_omits_unconfirmed_when_confirmed() {
        let transaction = QortalTransaction {
            block_height: 42,
            datetime: 1_700_000_000,
            txid: "00".repeat(32),
            amount: 25,
            fee: 0,
            incoming_metadata: vec![QortalTxMetadata {
                address: "zs1example".to_string(),
                value: 25,
                memo: None,
            }],
            incoming_metadata_change: Vec::new(),
            outgoing_metadata: Vec::new(),
            outgoing_metadata_change: Vec::new(),
            unconfirmed: None,
        };

        let encoded = serde_json::to_value(transaction).unwrap();
        assert!(encoded.get("unconfirmed").is_none());
        assert_eq!(encoded["incoming_metadata"][0]["value"], 25);
    }

    #[test]
    fn sync_session_uses_numeric_ids_and_relative_progress() {
        let mut session = SyncSession::default();
        let mut status = SyncStatus {
            local_height: 100,
            target_height: 200,
            percent: 50.0,
            eta: None,
            stage: SyncStage::Notes,
            last_checkpoint: None,
            blocks_per_second: 0.0,
            notes_decrypted: 0,
            last_batch_ms: 0,
        };

        let first = update_sync_session(&mut session, &status);
        assert_eq!(first.sync_id, 1);
        assert_eq!(first.start_block, Some(100));
        assert_eq!(first.synced_blocks, Some(0));
        assert_eq!(first.total_blocks, Some(100));

        status.local_height = 140;
        let progress = update_sync_session(&mut session, &status);
        assert_eq!(progress.sync_id, 1);
        assert_eq!(progress.synced_blocks, Some(40));
        assert_eq!(progress.trial_decryptions_blocks, Some(40));
        assert_eq!(progress.txn_scan_blocks, Some(40));

        status.local_height = 200;
        let idle = update_sync_session(&mut session, &status);
        assert!(!idle.in_progress);
        assert_eq!(idle.scanned_height, Some(200));
        assert!(idle.start_block.is_none());

        status.local_height = 200;
        status.target_height = 250;
        let next = update_sync_session(&mut session, &status);
        assert_eq!(next.sync_id, 2);
    }
}
