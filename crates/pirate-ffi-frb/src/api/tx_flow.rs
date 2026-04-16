use super::*;

fn extract_orchard_anchor_from_raw_tx(raw_tx_bytes: &[u8]) -> Option<[u8; 32]> {
    let tx = Transaction::read(raw_tx_bytes, BranchId::Nu5)
        .or_else(|_| Transaction::read(raw_tx_bytes, BranchId::Canopy))
        .ok()?;
    tx.orchard_bundle().map(|bundle| bundle.anchor().to_bytes())
}

fn extract_sapling_anchor_from_raw_tx(raw_tx_bytes: &[u8]) -> Option<[u8; 32]> {
    let tx = Transaction::read(raw_tx_bytes, BranchId::Nu5)
        .or_else(|_| Transaction::read(raw_tx_bytes, BranchId::Canopy))
        .ok()?;
    let bundle = tx.sapling_bundle()?;
    bundle
        .shielded_spends()
        .first()
        .map(|spend| spend.anchor().to_bytes())
}

fn parse_sapling_root_from_tree_state(
    tree_state: &pirate_sync_lightd::client::TreeState,
) -> Option<[u8; 32]> {
    let encoded = if !tree_state.sapling_frontier.is_empty() {
        tree_state.sapling_frontier.trim()
    } else if !tree_state.sapling_tree.is_empty() {
        tree_state.sapling_tree.trim()
    } else {
        return None;
    };
    if encoded.is_empty() {
        return None;
    }
    if encoded.len() == 64 {
        return hex::decode(encoded)
            .ok()
            .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok());
    }
    let bytes = hex::decode(encoded).ok()?;
    if let Ok(tree) = read_commitment_tree::<
        zcash_primitives::sapling::Node,
        _,
        { zcash_primitives::sapling::NOTE_COMMITMENT_TREE_DEPTH },
    >(&bytes[..])
    {
        return Some(tree.root().to_bytes());
    }
    let frontier = read_frontier_v1::<zcash_primitives::sapling::Node, _>(&bytes[..])
        .or_else(|_| read_frontier_v0::<zcash_primitives::sapling::Node, _>(&bytes[..]))
        .ok()?;
    Some(frontier.root().to_bytes())
}

fn parse_orchard_root_from_tree_state(
    tree_state: &pirate_sync_lightd::client::TreeState,
) -> Option<[u8; 32]> {
    let encoded = tree_state.orchard_tree.trim();
    if encoded.is_empty() {
        return None;
    }
    if encoded.len() == 64 {
        return hex::decode(encoded)
            .ok()
            .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok());
    }

    let bytes = hex::decode(encoded).ok()?;
    if let Ok(tree) = read_commitment_tree::<
        orchard::tree::MerkleHashOrchard,
        _,
        { zcash_primitives::sapling::NOTE_COMMITMENT_TREE_DEPTH },
    >(&bytes[..])
    {
        return Some(tree.root().to_bytes());
    }

    let frontier = read_frontier_v1::<orchard::tree::MerkleHashOrchard, _>(&bytes[..])
        .or_else(|_| read_frontier_v0::<orchard::tree::MerkleHashOrchard, _>(&bytes[..]))
        .ok()?;
    Some(frontier.root().to_bytes())
}

fn encode_hex_opt(bytes: Option<[u8; 32]>) -> String {
    bytes.map(hex::encode).unwrap_or_else(|| "none".to_string())
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SpendSelectionAnchors {
    pub(super) target_height: u64,
    pub(super) conservative_anchor_height: u64,
    pub(super) sapling_anchor_height: u64,
    pub(super) orchard_anchor_height: u64,
}

fn compute_spend_selection_anchors(
    db: &Database,
    account_id: i64,
) -> Result<SpendSelectionAnchors> {
    let spendability_storage = SpendabilityStateStorage::new(db);
    let anchors = spendability_storage
        .get_target_and_anchor_heights_by_pool_for_account(
            SPENDABILITY_MIN_CONFIRMATIONS,
            account_id,
        )?
        .ok_or_else(|| anyhow!("Anchor height unavailable for spend selection"))?;
    Ok(SpendSelectionAnchors {
        target_height: anchors.target_height,
        conservative_anchor_height: anchors.conservative_anchor_height.max(1),
        sapling_anchor_height: anchors.sapling_anchor_height.max(1),
        orchard_anchor_height: anchors.orchard_anchor_height.max(1),
    })
}

fn load_selectable_notes_for_send(
    repo: &Repository,
    account_id: i64,
    anchors: SpendSelectionAnchors,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<Vec<pirate_core::selection::SelectableNote>> {
    if anchors.sapling_anchor_height == anchors.orchard_anchor_height {
        return Ok(repo.get_unspent_selectable_notes_at_anchor_filtered(
            account_id,
            anchors.conservative_anchor_height,
            SPENDABILITY_MIN_CONFIRMATIONS,
            key_ids_filter,
            address_ids_filter,
        )?);
    }

    let mut combined = Vec::new();
    let mut seen: HashSet<(Vec<u8>, u32, u8)> = HashSet::new();
    for note in repo.get_unspent_selectable_notes_at_anchor_filtered(
        account_id,
        anchors.sapling_anchor_height,
        SPENDABILITY_MIN_CONFIRMATIONS,
        key_ids_filter.clone(),
        address_ids_filter.clone(),
    )? {
        if note.note_type != pirate_core::selection::NoteType::Sapling {
            continue;
        }
        let key = (note.txid.clone(), note.output_index, 0u8);
        if seen.insert(key) {
            combined.push(note);
        }
    }
    for note in repo.get_unspent_selectable_notes_at_anchor_filtered(
        account_id,
        anchors.orchard_anchor_height,
        SPENDABILITY_MIN_CONFIRMATIONS,
        key_ids_filter,
        address_ids_filter,
    )? {
        if note.note_type != pirate_core::selection::NoteType::Orchard {
            continue;
        }
        let key = (note.txid.clone(), note.output_index, 1u8);
        if seen.insert(key) {
            combined.push(note);
        }
    }

    Ok(combined)
}

pub(super) fn normalize_filter_ids(ids: Option<Vec<i64>>) -> Option<Vec<i64>> {
    let values = ids?;
    let mut unique = HashSet::new();
    let mut normalized = Vec::new();
    for id in values {
        if unique.insert(id) {
            normalized.push(id);
        }
    }
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn validate_spendable_key(repo: &Repository, account_id: i64, key_id: i64) -> Result<()> {
    let key = repo
        .get_account_key_by_id(key_id)?
        .ok_or_else(|| anyhow!("Key group not found"))?;
    if key.account_id != account_id {
        return Err(anyhow!("Key group does not belong to this wallet"));
    }
    if !key.spendable {
        return Err(anyhow!("Key group is not spendable"));
    }
    Ok(())
}

pub(super) fn resolve_spend_key_id(
    repo: &Repository,
    account_id: i64,
    key_ids_filter: Option<&[i64]>,
    address_ids_filter: Option<&[i64]>,
) -> Result<Option<i64>> {
    let mut selected_key_id: Option<i64> = None;

    if let Some(ids) = key_ids_filter {
        if !ids.is_empty() {
            let unique: HashSet<i64> = ids.iter().copied().collect();
            if unique.len() > 1 {
                for key_id in unique {
                    validate_spendable_key(repo, account_id, key_id)?;
                }
                selected_key_id = None;
            } else {
                let key_id = *unique.iter().next().unwrap();
                validate_spendable_key(repo, account_id, key_id)?;
                selected_key_id = Some(key_id);
            }
        }
    }

    if let Some(address_ids) = address_ids_filter {
        if !address_ids.is_empty() {
            let addresses = repo.get_all_addresses(account_id)?;
            let mut address_key_ids = HashSet::new();
            for address_id in address_ids {
                let addr = addresses
                    .iter()
                    .find(|addr| addr.id == Some(*address_id))
                    .ok_or_else(|| anyhow!("Address {} not found", address_id))?;
                let key_id = addr
                    .key_id
                    .ok_or_else(|| anyhow!("Address {} is missing key id", address_id))?;
                address_key_ids.insert(key_id);
            }
            if address_key_ids.len() > 1 {
                for key_id in &address_key_ids {
                    validate_spendable_key(repo, account_id, *key_id)?;
                }

                if let Some(existing) = selected_key_id {
                    if !address_key_ids.contains(&existing) {
                        return Err(anyhow!(
                            "Selected key group does not match selected addresses"
                        ));
                    }
                }
                selected_key_id = None;
            } else if let Some(address_key_id) = address_key_ids.iter().next().copied() {
                validate_spendable_key(repo, account_id, address_key_id)?;
                if let Some(existing) = selected_key_id {
                    if existing != address_key_id {
                        return Err(anyhow!(
                            "Selected key group does not match selected addresses"
                        ));
                    }
                } else {
                    selected_key_id = Some(address_key_id);
                }
            }
        }
    }

    Ok(selected_key_id)
}

pub(super) fn auto_select_spend_key_id_for_amount(
    repo: &Repository,
    account_id: i64,
    required_total: u64,
    anchors: SpendSelectionAnchors,
) -> Result<Option<i64>> {
    let spendable_keys = repo
        .get_account_keys(account_id)?
        .into_iter()
        .filter(|key| key.spendable && key.sapling_extsk.is_some())
        .filter_map(|key| key.id)
        .collect::<HashSet<_>>();

    if spendable_keys.is_empty() {
        return Ok(None);
    }

    let mut totals_by_key = HashMap::<i64, u64>::new();
    for key_id in spendable_keys {
        let notes =
            load_selectable_notes_for_send(repo, account_id, anchors, Some(vec![key_id]), None)?;
        let total = notes
            .iter()
            .fold(0u64, |acc, note| acc.saturating_add(note.value));
        if total > 0 {
            totals_by_key.insert(key_id, total);
        }
    }

    if totals_by_key.is_empty() {
        return Ok(None);
    }

    let mut qualifying = totals_by_key
        .iter()
        .filter_map(|(key_id, total)| {
            if *total >= required_total {
                Some((*key_id, *total))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if qualifying.is_empty() {
        return Ok(None);
    }

    qualifying.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    Ok(Some(qualifying[0].0))
}

pub(super) fn note_balances_by_key_id(
    notes: &[pirate_core::selection::SelectableNote],
) -> HashMap<i64, u64> {
    let mut balances = HashMap::<i64, u64>::new();
    for note in notes {
        let Some(key_id) = note.key_id else {
            continue;
        };
        let entry = balances.entry(key_id).or_insert(0);
        *entry = entry.saturating_add(note.value);
    }
    balances
}

pub(super) fn infer_contributing_key_ids_for_amount(
    notes: &[pirate_core::selection::SelectableNote],
    required_total: u64,
) -> HashSet<i64> {
    let mut note_refs = notes.iter().collect::<Vec<_>>();
    note_refs.sort_by(|a, b| a.value.cmp(&b.value).then_with(|| a.height.cmp(&b.height)));

    let mut total = 0u64;
    let mut contributing = HashSet::<i64>::new();
    for note in note_refs {
        if total >= required_total {
            break;
        }
        total = total.saturating_add(note.value);
        if let Some(key_id) = note.key_id {
            contributing.insert(key_id);
        }
    }

    contributing
}

pub(super) fn choose_multi_key_change_sink_key_id(
    account_keys_by_id: &HashMap<i64, AccountKey>,
    contributing_key_ids: &HashSet<i64>,
    balances_by_key: &HashMap<i64, u64>,
) -> Option<i64> {
    let mut seed_candidates = account_keys_by_id
        .iter()
        .filter_map(|(key_id, key)| {
            if key.spendable
                && key.key_type == KeyType::Seed
                && key.key_scope == KeyScope::Account
                && key.sapling_extsk.is_some()
            {
                Some(*key_id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    seed_candidates.sort_unstable();
    if let Some(seed_key_id) = seed_candidates.into_iter().next() {
        return Some(seed_key_id);
    }

    let mut ranked_candidates = contributing_key_ids
        .iter()
        .filter_map(|key_id| {
            let key = account_keys_by_id.get(key_id)?;
            if !key.spendable || key.sapling_extsk.is_none() {
                return None;
            }
            Some((*key_id, *balances_by_key.get(key_id).unwrap_or(&0)))
        })
        .collect::<Vec<_>>();
    ranked_candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked_candidates.first().map(|(key_id, _)| *key_id)
}

fn resolve_change_diversifier_index(
    repo: &Repository,
    account_id: i64,
    key_id: i64,
) -> Result<u32> {
    let existing_index = repo
        .get_addresses_by_key(account_id, key_id)?
        .into_iter()
        .filter(|addr| addr.address_scope == pirate_storage_sqlite::AddressScope::Internal)
        .map(|addr| addr.diversifier_index)
        .min();

    Ok(existing_index.unwrap_or(0))
}

const PENDING_SIGN_CONTEXT_TTL_MS: u64 = 10 * 60 * 1000;
const PENDING_SIGN_CONTEXT_MAX_ENTRIES: usize = 128;

#[derive(Debug)]
struct PendingSignContext {
    wallet_id: WalletId,
    required_total: u64,
    created_at_ms: u64,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
    selected_notes: Vec<pirate_core::selection::SelectableNote>,
}

lazy_static::lazy_static! {
    static ref PENDING_SIGN_CONTEXTS: RwLock<HashMap<String, PendingSignContext>> =
        RwLock::new(HashMap::new());
}

fn normalize_pending_sign_filter_ids(ids: Option<&Vec<i64>>) -> Option<Vec<i64>> {
    ids.map(|values| {
        let mut normalized = values.clone();
        normalized.sort_unstable();
        normalized.dedup();
        normalized
    })
}

fn store_pending_sign_context(pending_id: &str, context: PendingSignContext) {
    let now = unix_timestamp_millis();
    let mut cache = PENDING_SIGN_CONTEXTS.write();
    cache.retain(|_, existing| {
        now.saturating_sub(existing.created_at_ms) <= PENDING_SIGN_CONTEXT_TTL_MS
    });
    cache.insert(pending_id.to_string(), context);
    while cache.len() > PENDING_SIGN_CONTEXT_MAX_ENTRIES {
        let Some(oldest_key) = cache
            .iter()
            .min_by_key(|(_, ctx)| ctx.created_at_ms)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        cache.remove(&oldest_key);
    }
}

fn take_pending_sign_context(
    pending_id: &str,
    wallet_id: &WalletId,
    required_total: u64,
    key_ids_filter: Option<&Vec<i64>>,
    address_ids_filter: Option<&Vec<i64>>,
) -> Option<PendingSignContext> {
    let now = unix_timestamp_millis();
    let expected_key_ids = normalize_pending_sign_filter_ids(key_ids_filter);
    let expected_address_ids = normalize_pending_sign_filter_ids(address_ids_filter);
    let mut cache = PENDING_SIGN_CONTEXTS.write();
    cache.retain(|_, existing| {
        now.saturating_sub(existing.created_at_ms) <= PENDING_SIGN_CONTEXT_TTL_MS
    });
    let ctx = cache.remove(pending_id)?;
    if now.saturating_sub(ctx.created_at_ms) > PENDING_SIGN_CONTEXT_TTL_MS {
        return None;
    }
    if &ctx.wallet_id != wallet_id {
        return None;
    }
    if ctx.required_total != required_total {
        return None;
    }
    if ctx.key_ids_filter != expected_key_ids {
        return None;
    }
    if ctx.address_ids_filter != expected_address_ids {
        return None;
    }
    Some(ctx)
}

fn clear_pending_sign_context(pending_id: &str) {
    PENDING_SIGN_CONTEXTS.write().remove(pending_id);
}

#[derive(Debug)]
struct BroadcastContext {
    wallet_id: WalletId,
    account_id: i64,
    spent_nullifiers: Vec<Vec<u8>>,
    change_amount: u64,
    created_at_ms: u64,
}

#[derive(Debug, Clone)]
struct PendingChangeEntry {
    txid: String,
    change_amount: u64,
    broadcast_at_ms: u64,
}

const PENDING_CHANGE_TTL_MS: u64 = 30 * 60 * 1000;

lazy_static::lazy_static! {
    static ref PENDING_CHANGES: RwLock<HashMap<WalletId, Vec<PendingChangeEntry>>> =
        RwLock::new(HashMap::new());
}

fn normalize_txid_hex(txid: &str) -> Option<String> {
    let normalized = txid.trim().to_ascii_lowercase();
    if normalized.len() == 64 && normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(normalized)
    } else {
        None
    }
}

pub(super) fn txid_hex_variants_from_bytes(txid_bytes: &[u8]) -> Vec<String> {
    if txid_bytes.is_empty() {
        return Vec::new();
    }
    let direct = hex::encode(txid_bytes);
    if txid_bytes.len() != 32 {
        return vec![direct];
    }
    let mut reversed = txid_bytes.to_vec();
    reversed.reverse();
    let reversed_hex = hex::encode(reversed);
    if reversed_hex == direct {
        vec![direct]
    } else {
        vec![direct, reversed_hex]
    }
}

pub(super) fn add_pending_change(wallet_id: &WalletId, txid: &str, change_amount: u64) {
    if change_amount == 0 {
        return;
    }
    let Some(txid) = normalize_txid_hex(txid) else {
        return;
    };
    let now = unix_timestamp_millis();
    let mut cache = PENDING_CHANGES.write();
    let entries = cache.entry(wallet_id.clone()).or_default();
    entries.retain(|e| now.saturating_sub(e.broadcast_at_ms) <= PENDING_CHANGE_TTL_MS);
    if let Some(existing) = entries.iter_mut().find(|e| e.txid == txid) {
        existing.change_amount = change_amount;
        existing.broadcast_at_ms = now;
        return;
    }
    entries.push(PendingChangeEntry {
        txid,
        change_amount,
        broadcast_at_ms: now,
    });
}

#[cfg(test)]
pub(super) fn clear_pending_changes(wallet_id: &WalletId) {
    PENDING_CHANGES.write().remove(wallet_id);
}

#[cfg(test)]
pub(super) fn has_pending_changes(wallet_id: &WalletId) -> bool {
    PENDING_CHANGES.read().contains_key(wallet_id)
}

pub(super) fn resolve_pending_change(wallet_id: &WalletId, known_txids: &HashSet<String>) -> u64 {
    let now = unix_timestamp_millis();
    let mut cache = PENDING_CHANGES.write();
    let Some(entries) = cache.get_mut(wallet_id) else {
        return 0;
    };
    entries.retain(|e| {
        now.saturating_sub(e.broadcast_at_ms) <= PENDING_CHANGE_TTL_MS
            && !known_txids.contains(&e.txid)
    });
    let total: u64 = entries.iter().map(|e| e.change_amount).sum();
    if entries.is_empty() {
        cache.remove(wallet_id);
    }
    total
}

const BROADCAST_CONTEXT_TTL_MS: u64 = 30 * 60 * 1000;
const BROADCAST_CONTEXT_MAX_ENTRIES: usize = 64;

lazy_static::lazy_static! {
    static ref BROADCAST_CONTEXTS: RwLock<HashMap<String, BroadcastContext>> =
        RwLock::new(HashMap::new());
}

fn store_broadcast_context(txid: &str, context: BroadcastContext) {
    let now = unix_timestamp_millis();
    let mut cache = BROADCAST_CONTEXTS.write();
    cache.retain(|_, existing| {
        now.saturating_sub(existing.created_at_ms) <= BROADCAST_CONTEXT_TTL_MS
    });
    cache.insert(txid.to_string(), context);
    while cache.len() > BROADCAST_CONTEXT_MAX_ENTRIES {
        let Some(oldest_key) = cache
            .iter()
            .min_by_key(|(_, ctx)| ctx.created_at_ms)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        cache.remove(&oldest_key);
    }
}

fn take_broadcast_context(txid: &str) -> Option<BroadcastContext> {
    BROADCAST_CONTEXTS.write().remove(txid)
}

fn build_tx_internal(
    wallet_id: WalletId,
    outputs: Vec<Output>,
    fee_opt: Option<u64>,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<PendingTx> {
    tracing::info!(
        "Building transaction for wallet {} with {} outputs",
        wallet_id,
        outputs.len()
    );

    if outputs.is_empty() {
        return Err(anyhow!("At least one output is required"));
    }
    if outputs.len() > MAX_OUTPUTS_PER_TX {
        return Err(anyhow!(
            "Too many outputs: {} (maximum {})",
            outputs.len(),
            MAX_OUTPUTS_PER_TX
        ));
    }

    let mut has_memo = false;
    let mut total_amount = 0u64;

    for (i, output) in outputs.iter().enumerate() {
        output
            .validate()
            .map_err(|e| anyhow!("Output {}: {}", i + 1, e))?;

        let is_orchard = output.addr.starts_with("pirate1")
            || output.addr.starts_with("pirate-test1")
            || output.addr.starts_with("pirate-regtest1");
        let is_sapling = output.addr.starts_with("zs1")
            || output.addr.starts_with("ztestsapling1")
            || output.addr.starts_with("zregtestsapling1");

        if !is_orchard && !is_sapling {
            return Err(anyhow!(
                "Invalid address at output {}: must be Sapling (zs1...) or Orchard (pirate1...) address",
                i + 1
            ));
        }

        if is_orchard {
            OrchardPaymentAddress::decode_any_network(&output.addr)
                .map_err(|e| anyhow!("Invalid Orchard address at output {}: {}", i + 1, e))?;
        } else {
            PaymentAddress::decode_any_network(&output.addr)
                .map_err(|e| anyhow!("Invalid Sapling address at output {}: {}", i + 1, e))?;
        }

        if let Some(ref memo_text) = output.memo {
            let memo_bytes = memo_text.len();
            if memo_bytes > MAX_MEMO_LENGTH {
                return Err(anyhow!(
                    "Memo at output {} is too long: {} bytes (maximum {})",
                    i + 1,
                    memo_bytes,
                    MAX_MEMO_LENGTH
                ));
            }

            if memo_text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r')
            {
                return Err(anyhow!(
                    "Memo at output {} contains invalid control characters",
                    i + 1
                ));
            }

            has_memo = true;
        }

        total_amount = total_amount
            .checked_add(output.amount)
            .ok_or_else(|| anyhow!("Amount overflow"))?;
    }

    let fee_calculator = FeeCalculator::new();
    let calculated_fee = fee_calculator
        .calculate_fee(2, outputs.len(), has_memo)
        .map_err(|e| anyhow!("Fee calculation error: {}", e))?;
    let fee = fee_opt.unwrap_or(calculated_fee);

    fee_calculator
        .validate_fee(fee)
        .map_err(|e| anyhow!("Invalid fee: {}", e))?;

    let required_total = total_amount
        .checked_add(fee)
        .ok_or_else(|| anyhow!("Amount + fee overflow"))?;

    let key_ids_filter = normalize_filter_ids(key_ids_filter);
    let address_ids_filter = normalize_filter_ids(address_ids_filter);

    let (db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;
    let spendability = sync_control::require_spendability_ready_with_sync_trigger(&wallet_id)?;
    let anchors = compute_spend_selection_anchors(db, secret.account_id)?;
    let computed_target_height = anchors.target_height;
    let anchor_height = anchors.conservative_anchor_height;
    let resolved_key_id = resolve_spend_key_id(
        &repo,
        secret.account_id,
        key_ids_filter.as_deref(),
        address_ids_filter.as_deref(),
    )?;
    let mut effective_key_ids_filter = key_ids_filter.clone();
    if key_ids_filter.is_none() && address_ids_filter.is_none() {
        let auto_key_id = match resolved_key_id {
            Some(key_id) => Some(key_id),
            None => auto_select_spend_key_id_for_amount(
                &repo,
                secret.account_id,
                required_total,
                anchors,
            )?,
        };

        if let Some(key_id) = auto_key_id {
            effective_key_ids_filter = Some(vec![key_id]);
            tracing::info!(
                "Auto-selected key group {} for build_tx wallet {} (required={})",
                key_id,
                wallet_id,
                required_total
            );
        }
    }

    let selectable_notes_raw = load_selectable_notes_for_send(
        &repo,
        secret.account_id,
        anchors,
        effective_key_ids_filter.clone(),
        address_ids_filter.clone(),
    )?;
    let selectable_notes = selectable_notes_raw;
    let available_balance: u64 = selectable_notes.iter().map(|note| note.value).sum();
    let eligible_note_count = selectable_notes
        .iter()
        .filter(|note| note.auto_consolidation_eligible)
        .count();
    let auto_consolidate = auto_consolidation_enabled(&wallet_id).unwrap_or(false)
        && key_ids_filter.is_none()
        && address_ids_filter.is_none()
        && eligible_note_count >= AUTO_CONSOLIDATION_THRESHOLD;
    let auto_consolidation_extra_limit = if auto_consolidate {
        AUTO_CONSOLIDATION_MAX_EXTRA_NOTES
    } else {
        0
    };
    let key_ids_count = effective_key_ids_filter.as_ref().map_or(0, |ids| ids.len());
    let address_ids_count = address_ids_filter.as_ref().map_or(0, |ids| ids.len());
    let key_id_log = if key_ids_count == 1 {
        effective_key_ids_filter.as_ref().unwrap()[0]
    } else {
        -1
    };
    let address_id_log = if address_ids_count == 1 {
        address_ids_filter.as_ref().unwrap()[0]
    } else {
        -1
    };
    {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_build_tx","timestamp":{},"location":"api.rs:1920","message":"build_tx notes","data":{{"wallet_id":"{}","account_id":{},"key_id":{},"address_id":{},"key_ids_count":{},"address_ids_count":{},"selectable_notes":{},"available_balance":{},"total_amount":{},"fee":{},"anchor_height":{},"validated_anchor_height":{},"computed_target_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts,
                wallet_id,
                secret.account_id,
                key_id_log,
                address_id_log,
                key_ids_count,
                address_ids_count,
                selectable_notes.len(),
                available_balance,
                total_amount,
                fee,
                anchor_height,
                spendability.validated_anchor_height,
                computed_target_height
            );
        });
    }

    if required_total > available_balance {
        return Err(anyhow!(
            "Insufficient funds: need {} arrrtoshis, have {} arrrtoshis",
            required_total,
            available_balance
        ));
    }

    let selector = NoteSelector::new(SelectionStrategy::SmallestFirst);
    let selection = if auto_consolidation_extra_limit > 0 {
        selector
            .select_notes_with_consolidation(
                selectable_notes,
                total_amount,
                fee,
                auto_consolidation_extra_limit,
            )
            .map_err(|e| anyhow!("Note selection failed: {}", e))?
    } else {
        selector
            .select_notes(selectable_notes, total_amount, fee)
            .map_err(|e| anyhow!("Note selection failed: {}", e))?
    };
    let pirate_core::selection::SelectionResult {
        notes: selected_notes,
        total_value: selected_input_total,
        change: selected_change,
    } = selection;

    let effective = apply_dust_policy_add_to_fee(fee, selected_change)
        .map_err(|e| anyhow!("Dust policy resolution failed: {}", e))?;
    let effective_fee = effective.fee;
    let change = effective.change;
    if effective.dust_added_to_fee > 0 {
        tracing::info!(
            "Applied dust-to-fee policy for pending tx {}: dust={} fee={} change={}",
            wallet_id,
            effective.dust_added_to_fee,
            effective_fee,
            change
        );
    }
    let pending_required_total = total_amount
        .checked_add(effective_fee)
        .ok_or_else(|| anyhow!("Amount + effective fee overflow"))?;

    let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(db);
    let sync_state = sync_storage.load_sync_state()?;
    let current_height = sync_state.local_height as u32;
    let expiry_height = current_height.saturating_add(40);

    let pending = PendingTx {
        id: uuid::Uuid::new_v4().to_string(),
        outputs,
        total_amount,
        fee: effective_fee,
        change,
        input_total: selected_input_total,
        num_inputs: selected_notes.len() as u32,
        expiry_height,
        created_at: chrono::Utc::now().timestamp(),
    };

    store_pending_sign_context(
        &pending.id,
        PendingSignContext {
            wallet_id: wallet_id.clone(),
            required_total: pending_required_total,
            created_at_ms: unix_timestamp_millis(),
            key_ids_filter: normalize_pending_sign_filter_ids(effective_key_ids_filter.as_ref()),
            address_ids_filter: normalize_pending_sign_filter_ids(address_ids_filter.as_ref()),
            selected_notes,
        },
    );

    tracing::info!(
        "Built pending tx {}: {} outputs, {} fee, {} change",
        pending.id,
        pending.outputs.len(),
        pending.fee,
        pending.change
    );

    Ok(pending)
}

pub(super) fn build_tx(
    wallet_id: WalletId,
    outputs: Vec<Output>,
    fee_opt: Option<u64>,
) -> Result<PendingTx> {
    build_tx_internal(wallet_id, outputs, fee_opt, None, None)
}

pub(super) fn build_tx_for_key(
    wallet_id: WalletId,
    key_id: i64,
    outputs: Vec<Output>,
    fee_opt: Option<u64>,
) -> Result<PendingTx> {
    build_tx_internal(wallet_id, outputs, fee_opt, Some(vec![key_id]), None)
}

pub(super) fn build_tx_filtered(
    wallet_id: WalletId,
    outputs: Vec<Output>,
    fee_opt: Option<u64>,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<PendingTx> {
    build_tx_internal(
        wallet_id,
        outputs,
        fee_opt,
        key_ids_filter,
        address_ids_filter,
    )
}

pub(super) fn build_consolidation_tx(
    wallet_id: WalletId,
    key_id: i64,
    target_address: String,
    fee_opt: Option<u64>,
) -> Result<PendingTx> {
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let _spendability = sync_control::require_spendability_ready_with_sync_trigger(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;
    let anchors = compute_spend_selection_anchors(_db, secret.account_id)?;
    let selectable_notes_raw = load_selectable_notes_for_send(
        &repo,
        secret.account_id,
        anchors,
        Some(vec![key_id]),
        None,
    )?;
    let selectable_notes = selectable_notes_raw;
    let available_balance: u64 = selectable_notes.iter().map(|note| note.value).sum();

    if available_balance == 0 {
        return Err(anyhow!("No spendable notes available for consolidation"));
    }

    let fee_calculator = FeeCalculator::new();
    let calculated_fee = fee_calculator
        .calculate_fee(1, 1, false)
        .map_err(|e| anyhow!("Fee calculation error: {}", e))?;
    let fee = fee_opt.unwrap_or(calculated_fee);
    fee_calculator
        .validate_fee(fee)
        .map_err(|e| anyhow!("Invalid fee: {}", e))?;

    if available_balance <= fee {
        return Err(anyhow!(
            "Insufficient funds: need {} arrrtoshis for fee, have {} arrrtoshis",
            fee,
            available_balance
        ));
    }

    let outputs = vec![Output {
        addr: target_address,
        amount: available_balance - fee,
        memo: None,
    }];

    build_tx_internal(wallet_id, outputs, Some(fee), Some(vec![key_id]), None)
}

pub(super) fn build_sweep_tx(
    wallet_id: WalletId,
    target_address: String,
    fee_opt: Option<u64>,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<PendingTx> {
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let _spendability = sync_control::require_spendability_ready_with_sync_trigger(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;
    let anchors = compute_spend_selection_anchors(_db, secret.account_id)?;

    let key_ids_filter = normalize_filter_ids(key_ids_filter);
    let address_ids_filter = normalize_filter_ids(address_ids_filter);
    let _resolved_key_id = resolve_spend_key_id(
        &repo,
        secret.account_id,
        key_ids_filter.as_deref(),
        address_ids_filter.as_deref(),
    )?;

    let selectable_notes_raw = load_selectable_notes_for_send(
        &repo,
        secret.account_id,
        anchors,
        key_ids_filter.clone(),
        address_ids_filter.clone(),
    )?;
    let selectable_notes = selectable_notes_raw;
    let available_balance: u64 = selectable_notes.iter().map(|note| note.value).sum();

    if available_balance == 0 {
        return Err(anyhow!("No spendable notes available for sweep"));
    }

    let fee_calculator = FeeCalculator::new();
    let calculated_fee = fee_calculator
        .calculate_fee(1, 1, false)
        .map_err(|e| anyhow!("Fee calculation error: {}", e))?;
    let fee = fee_opt.unwrap_or(calculated_fee);
    fee_calculator
        .validate_fee(fee)
        .map_err(|e| anyhow!("Invalid fee: {}", e))?;

    if available_balance <= fee {
        return Err(anyhow!(
            "Insufficient funds: need {} arrrtoshis for fee, have {} arrrtoshis",
            fee,
            available_balance
        ));
    }

    let outputs = vec![Output {
        addr: target_address,
        amount: available_balance - fee,
        memo: None,
    }];

    build_tx_internal(
        wallet_id,
        outputs,
        Some(fee),
        key_ids_filter,
        address_ids_filter,
    )
}

fn sign_tx_internal(
    wallet_id: WalletId,
    pending: PendingTx,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<SignedTx> {
    tracing::info!(
        "Signing transaction {} for wallet {}",
        pending.id,
        wallet_id
    );
    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_sign_tx_start","timestamp":{},"location":"api.rs:2019","message":"sign_tx start","data":{{"wallet_id":"{}","pending_id":"{}","outputs":{},"total_amount":{},"fee":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
            ts,
            wallet_id,
            pending.id,
            pending.outputs.len(),
            pending.total_amount,
            pending.fee
        );
    });

    let log_step = |step: &str, detail: &str| {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_sign_tx_step","timestamp":{},"location":"api.rs:2035","message":"sign_tx step","data":{{"wallet_id":"{}","step":"{}","detail":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                ts, wallet_id, step, detail
            );
        });
    };
    let mut pending = pending;
    let normalized = apply_dust_policy_add_to_fee(pending.fee, pending.change)
        .map_err(|e| anyhow!("Pending tx dust normalization failed: {}", e))?;
    if normalized.fee != pending.fee || normalized.change != pending.change {
        log_step(
            "pending_dust_normalized",
            &format!(
                "orig_fee={},orig_change={},new_fee={},new_change={},dust_added={}",
                pending.fee,
                pending.change,
                normalized.fee,
                normalized.change,
                normalized.dust_added_to_fee
            ),
        );
        pending.fee = normalized.fee;
        pending.change = normalized.change;
    }

    log_step("open_db_start", "");
    let (_db, repo) = open_wallet_db_for(&wallet_id).map_err(|e| {
        log_step("open_db_error", &format!("{:?}", e));
        e
    })?;
    log_step("open_db_ok", "");
    log_step("load_wallet_secret_start", "");
    let secret = repo.get_wallet_secret(&wallet_id)?.ok_or_else(|| {
        log_step("load_wallet_secret_error", "missing");
        anyhow!("No wallet secret found for {}", wallet_id)
    })?;
    log_step("load_wallet_secret_ok", "");

    let key_ids_filter = normalize_filter_ids(key_ids_filter);
    let address_ids_filter = normalize_filter_ids(address_ids_filter);
    let required_total = pending
        .total_amount
        .checked_add(pending.fee)
        .ok_or_else(|| anyhow!("Amount + fee overflow"))?;
    let spendability = sync_control::require_spendability_ready_with_sync_trigger(&wallet_id)?;
    let anchors = compute_spend_selection_anchors(_db, secret.account_id)?;
    let anchor_height = anchors.conservative_anchor_height;
    let orchard_anchor_height = anchors.orchard_anchor_height;
    log_step(
        "spendability_status",
        &format!(
            "spendable={},rescan_required={},target_height={},anchor_height={},validated_anchor_height={},repair_queued={},reason_code={}",
            spendability.spendable,
            spendability.rescan_required,
            spendability.target_height,
            spendability.anchor_height,
            spendability.validated_anchor_height,
            spendability.repair_queued,
            spendability.reason_code
        ),
    );
    let mut signing_key_id = resolve_spend_key_id(
        &repo,
        secret.account_id,
        key_ids_filter.as_deref(),
        address_ids_filter.as_deref(),
    )?;
    let mut effective_key_ids_filter = key_ids_filter.clone();
    if key_ids_filter.is_none() && address_ids_filter.is_none() {
        if signing_key_id.is_none() {
            signing_key_id = auto_select_spend_key_id_for_amount(
                &repo,
                secret.account_id,
                required_total,
                anchors,
            )?;
        }
        if let Some(key_id) = signing_key_id {
            effective_key_ids_filter = Some(vec![key_id]);
            log_step("auto_key_selected", &format!("key_id={}", key_id));
        }
    }
    log_step(
        "signing_key_resolution",
        &format!(
            "signing_key_id={},effective_key_ids_count={},address_ids_count={}",
            signing_key_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "none".to_string()),
            effective_key_ids_filter.as_ref().map_or(0, |ids| ids.len()),
            address_ids_filter.as_ref().map_or(0, |ids| ids.len())
        ),
    );

    let (mut sapling_extsk_bytes, mut orchard_extsk_bytes, mut change_key_id) =
        if let Some(key_id) = signing_key_id {
            let key = repo
                .get_account_key_by_id(key_id)?
                .ok_or_else(|| anyhow!("Key group not found"))?;
            if key.account_id != secret.account_id {
                return Err(anyhow!("Key group does not belong to this wallet"));
            }
            if !key.spendable {
                return Err(anyhow!("Key group is not spendable"));
            }
            let sapling_bytes = key
                .sapling_extsk
                .clone()
                .ok_or_else(|| anyhow!("Sapling spending key missing for key group"))?;
            (sapling_bytes, key.orchard_extsk.clone(), key_id)
        } else {
            (
                secret.extsk.clone(),
                secret.orchard_extsk.clone(),
                ensure_primary_account_key(&repo, &wallet_id, &secret)?,
            )
        };

    let mut account_keys_by_id: HashMap<i64, AccountKey> = HashMap::new();
    let mut sapling_spend_keys_by_id: HashMap<i64, ExtendedSpendingKey> = HashMap::new();
    let mut orchard_spend_keys_by_id: HashMap<i64, OrchardExtendedSpendingKey> = HashMap::new();
    if let Ok(account_keys) = repo.get_account_keys(secret.account_id) {
        for key in account_keys {
            if !key.spendable {
                continue;
            }
            let Some(key_id) = key.id else {
                continue;
            };
            account_keys_by_id.insert(key_id, key.clone());
            if let Some(bytes) = key.sapling_extsk.as_ref() {
                if let Ok(parsed) = ExtendedSpendingKey::from_bytes(bytes) {
                    sapling_spend_keys_by_id.insert(key_id, parsed);
                }
            }
            if let Some(bytes) = key.orchard_extsk.as_ref() {
                if let Ok(parsed) = OrchardExtendedSpendingKey::from_bytes(bytes) {
                    orchard_spend_keys_by_id.insert(key_id, parsed);
                }
            }
        }
    }

    let mut selectable_notes = if let Some(context) = take_pending_sign_context(
        &pending.id,
        &wallet_id,
        required_total,
        effective_key_ids_filter.as_ref(),
        address_ids_filter.as_ref(),
    ) {
        log_step(
            "pending_context_applied",
            &format!("selected_notes={}", context.selected_notes.len()),
        );
        context.selected_notes
    } else {
        log_step(
            "pending_context_miss",
            "reason=not_found_or_stale_or_mismatch",
        );
        return Err(anyhow!(
            "Pending send context expired. Rebuild transaction and retry."
        ));
    };

    let mut forced_auto_consolidation_extra_limit = 0usize;
    if key_ids_filter.is_none() && address_ids_filter.is_none() && signing_key_id.is_none() {
        let contributing_key_ids =
            infer_contributing_key_ids_for_amount(&selectable_notes, required_total);
        if contributing_key_ids.len() > 1 {
            let balances_by_key = note_balances_by_key_id(&selectable_notes);
            if let Some(change_sink_key_id) = choose_multi_key_change_sink_key_id(
                &account_keys_by_id,
                &contributing_key_ids,
                &balances_by_key,
            ) {
                if let Some(change_sink_key) = account_keys_by_id.get(&change_sink_key_id) {
                    if let Some(sapling_bytes) = change_sink_key.sapling_extsk.clone() {
                        sapling_extsk_bytes = sapling_bytes;
                        orchard_extsk_bytes = change_sink_key.orchard_extsk.clone();
                        change_key_id = change_sink_key_id;
                        log_step(
                            "multi_key_change_sink_selected",
                            &format!(
                                "change_key_id={},contributors={}",
                                change_sink_key_id,
                                contributing_key_ids.len()
                            ),
                        );
                    }
                }
            }

            let imported_contributing_key_ids = contributing_key_ids
                .iter()
                .filter_map(|key_id| {
                    let key = account_keys_by_id.get(key_id)?;
                    if key.key_type == KeyType::ImportSpend {
                        Some(*key_id)
                    } else {
                        None
                    }
                })
                .collect::<HashSet<_>>();

            if !imported_contributing_key_ids.is_empty() {
                let mut marked_notes = 0usize;
                for note in &mut selectable_notes {
                    if note
                        .key_id
                        .is_some_and(|key_id| imported_contributing_key_ids.contains(&key_id))
                    {
                        if !note.auto_consolidation_eligible {
                            note.auto_consolidation_eligible = true;
                        }
                        marked_notes += 1;
                    }
                }
                forced_auto_consolidation_extra_limit = marked_notes;
                log_step(
                    "multi_key_import_sweep_marked",
                    &format!(
                        "contributors={},imported_groups={},marked_notes={}",
                        contributing_key_ids.len(),
                        imported_contributing_key_ids.len(),
                        marked_notes
                    ),
                );
            }
        }
    }

    let extsk = ExtendedSpendingKey::from_bytes(&sapling_extsk_bytes).map_err(|e| {
        log_step("extsk_parse_error", &format!("{:?}", e));
        anyhow!("Invalid spending key bytes: {}", e)
    })?;
    log_step("extsk_parse_ok", "");

    let orchard_extsk_opt = orchard_extsk_bytes
        .as_ref()
        .and_then(|bytes| OrchardExtendedSpendingKey::from_bytes(bytes).ok());

    if signing_key_id.is_some()
        && orchard_extsk_opt.is_none()
        && selectable_notes
            .iter()
            .any(|note| note.note_type == pirate_core::selection::NoteType::Orchard)
    {
        log_step("orchard_extsk_missing", "");
        return Err(anyhow!("Orchard spending key missing for this key group"));
    }

    let eligible_note_count = selectable_notes
        .iter()
        .filter(|note| note.auto_consolidation_eligible)
        .count();
    let mut auto_consolidation_extra_limit = 0usize;
    if auto_consolidation_enabled(&wallet_id).unwrap_or(false)
        && effective_key_ids_filter.is_none()
        && address_ids_filter.is_none()
        && eligible_note_count >= AUTO_CONSOLIDATION_THRESHOLD
    {
        auto_consolidation_extra_limit = AUTO_CONSOLIDATION_MAX_EXTRA_NOTES;
    }
    if forced_auto_consolidation_extra_limit > auto_consolidation_extra_limit {
        auto_consolidation_extra_limit = forced_auto_consolidation_extra_limit;
    }

    let wallet_meta = get_wallet_meta(&wallet_id)?;
    let network_type_str = wallet_meta.network_type.as_deref().unwrap_or("mainnet");
    let network_type = match network_type_str {
        "testnet" => NetworkType::Testnet,
        "regtest" => NetworkType::Regtest,
        _ => NetworkType::Mainnet,
    };

    let mut builder = pirate_core::shielded_builder::ShieldedBuilder::with_network(network_type);
    builder.with_fee_per_action(pending.fee);
    if auto_consolidation_extra_limit > 0 {
        builder.with_auto_consolidation_extra_limit(auto_consolidation_extra_limit);
    }
    let mut has_orchard_output = false;

    for out in &pending.outputs {
        let is_orchard = out.addr.starts_with("pirate1")
            || out.addr.starts_with("pirate-test1")
            || out.addr.starts_with("pirate-regtest1");

        let memo = out
            .memo
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| pirate_core::memo::Memo::from_text_truncated(s.clone()));

        if is_orchard {
            has_orchard_output = true;
            let addr = OrchardPaymentAddress::decode_any_network(&out.addr)
                .map_err(|e| anyhow!("Invalid Orchard address {}: {}", out.addr, e))?;
            builder.add_orchard_output(addr.inner, out.amount, memo)?;
        } else {
            let addr = PaymentAddress::decode_any_network(&out.addr)
                .map_err(|e| anyhow!("Invalid Sapling address {}: {}", out.addr, e))?;
            builder.add_sapling_output(addr, out.amount, memo)?;
        }
    }

    let mut note_refs: Vec<&pirate_core::selection::SelectableNote> =
        selectable_notes.iter().collect();
    note_refs.sort_by(|a, b| a.value.cmp(&b.value));
    let mut total_selected = 0u64;
    let mut extra_selected = 0usize;
    let mut has_orchard_spends = false;
    for note in note_refs {
        if total_selected < required_total {
            total_selected = total_selected
                .checked_add(note.value)
                .ok_or_else(|| anyhow!("Value overflow"))?;
            if note.note_type == pirate_core::selection::NoteType::Orchard {
                has_orchard_spends = true;
            }
            continue;
        }

        if auto_consolidation_extra_limit == 0 || extra_selected >= auto_consolidation_extra_limit {
            break;
        }

        if note.auto_consolidation_eligible {
            total_selected = total_selected
                .checked_add(note.value)
                .ok_or_else(|| anyhow!("Value overflow"))?;
            extra_selected += 1;
            if note.note_type == pirate_core::selection::NoteType::Orchard {
                has_orchard_spends = true;
            }
        }
    }
    let use_orchard_change = has_orchard_output || has_orchard_spends;

    if spendability.target_height == 0 {
        return Err(anyhow!(
            "{}: Spendability target height is not initialized yet.",
            SPENDABILITY_REASON_ERR_SYNC_FINALIZING
        ));
    }
    let target_height_u64 = spendability.target_height.max(1);
    let target_height = u32::try_from(target_height_u64)
        .map_err(|_| anyhow!("Target height {} exceeds u32 range", target_height_u64))?;
    let use_sapling_internal_change = !use_orchard_change
        && pirate_core::sapling_internal_change_active(network_type, target_height_u64);
    log_step(
        "load_sync_state_ok",
        &format!(
            "target_height={},anchor_height={},validated_anchor_height={},sapling_internal_change={}",
            target_height,
            spendability.anchor_height,
            spendability.validated_anchor_height,
            use_sapling_internal_change
        ),
    );

    let orchard_anchor_opt = if has_orchard_spends {
        None
    } else if has_orchard_output {
        log_step("orchard_anchor_fetch_start", "outputs_only");

        let anchor_opt = repo
            .resolve_orchard_anchor_from_db_state(orchard_anchor_height)
            .map_err(|e| anyhow!("Failed to resolve Orchard anchor from DB state: {}", e))?;

        if anchor_opt.is_some() {
            log_step(
                "orchard_anchor_fetch_ok",
                &format!("anchor_height<={}", orchard_anchor_height),
            );
        } else {
            log_step("orchard_anchor_sync_required", "missing_db_anchor_state");
            return Err(anyhow!(
                "Sync required: Orchard anchor is not available locally yet. Let wallet sync complete, then retry."
            ));
        }

        anchor_opt
    } else {
        None
    };

    let selected_orchard_anchor_hex = selectable_notes
        .iter()
        .find(|note| note.note_type == pirate_core::selection::NoteType::Orchard)
        .and_then(|note| note.orchard_anchor)
        .map(|anchor| anchor.to_bytes());
    log_step(
        "orchard_anchor_selected",
        &format!(
            "anchor_height={},has_orchard_spends={},has_orchard_output={},note_anchor={},provided_anchor={}",
            anchor_height,
            has_orchard_spends,
            has_orchard_output,
            encode_hex_opt(selected_orchard_anchor_hex),
            encode_hex_opt(orchard_anchor_opt.map(|a| a.to_bytes())),
        ),
    );

    let change_diversifier_index =
        resolve_change_diversifier_index(&repo, secret.account_id, change_key_id).map_err(|e| {
            log_step("change_diversifier_error", &format!("{:?}", e));
            e
        })?;
    log_step(
        "change_diversifier_ok",
        &format!("{}", change_diversifier_index),
    );

    if pending.change >= CHANGE_DUST_THRESHOLD
        && (use_orchard_change || use_sapling_internal_change)
    {
        let (change_addr, address_type) = if use_orchard_change {
            let orchard_extsk = orchard_extsk_opt
                .as_ref()
                .ok_or_else(|| anyhow!("Orchard spending key required for Orchard change"))?;
            let orchard_fvk = orchard_extsk.to_extended_fvk();
            let addr = orchard_fvk
                .address_at_internal(change_diversifier_index)
                .encode_for_network(network_type)?;
            (addr, AddressType::Orchard)
        } else {
            let addr = extsk
                .to_internal_fvk()
                .derive_address(change_diversifier_index)
                .encode_for_network(network_type);
            (addr, AddressType::Sapling)
        };

        let address = pirate_storage_sqlite::Address {
            id: None,
            key_id: Some(change_key_id),
            account_id: secret.account_id,
            diversifier_index: change_diversifier_index,
            address: change_addr,
            address_type,
            label: None,
            created_at: chrono::Utc::now().timestamp(),
            color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
            address_scope: pirate_storage_sqlite::AddressScope::Internal,
        };
        let _ = repo.upsert_address(&address);
    }

    let mut missing_sapling_key_ids: HashSet<i64> = HashSet::new();
    let mut missing_orchard_key_ids: HashSet<i64> = HashSet::new();
    for note in &selectable_notes {
        if let Some(key_id) = note.key_id {
            match note.note_type {
                pirate_core::selection::NoteType::Sapling => {
                    if !sapling_spend_keys_by_id.contains_key(&key_id) {
                        missing_sapling_key_ids.insert(key_id);
                    }
                }
                pirate_core::selection::NoteType::Orchard => {
                    if !orchard_spend_keys_by_id.contains_key(&key_id) {
                        missing_orchard_key_ids.insert(key_id);
                    }
                }
            }
        }
    }
    if !missing_sapling_key_ids.is_empty() || !missing_orchard_key_ids.is_empty() {
        let mut sapling_ids = missing_sapling_key_ids.into_iter().collect::<Vec<_>>();
        let mut orchard_ids = missing_orchard_key_ids.into_iter().collect::<Vec<_>>();
        sapling_ids.sort_unstable();
        orchard_ids.sort_unstable();
        log_step(
            "spend_key_map_missing",
            &format!(
                "sapling_missing={:?},orchard_missing={:?}",
                sapling_ids, orchard_ids
            ),
        );
        return Err(anyhow!(
            "Selected notes require unavailable spending keys (sapling={:?}, orchard={:?}). Re-import the missing key group(s) and retry.",
            sapling_ids,
            orchard_ids
        ));
    }

    log_step("build_and_sign_start", "");
    let (build_tx, build_rx) = std::sync::mpsc::channel();
    let wallet_id_for_log = wallet_id.clone();
    let build_timeout = std::time::Duration::from_secs(120);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            futures::executor::block_on(builder.build_and_sign_multi(
                pirate_core::shielded_builder::BuildAndSignMultiInputs {
                    default_sapling_spending_key: &extsk,
                    default_orchard_spending_key: orchard_extsk_opt.as_ref(),
                    sapling_spending_keys_by_id: sapling_spend_keys_by_id,
                    orchard_spending_keys_by_id: orchard_spend_keys_by_id,
                    available_notes: selectable_notes,
                    target_height,
                    orchard_anchor: orchard_anchor_opt,
                    change_diversifier_index,
                },
            ))
            .map_err(|e| anyhow!("Build/sign failed: {}", e))
        }));
        let send_result: anyhow::Result<_> = match result {
            Ok(build_result) => build_result,
            Err(panic_payload) => {
                let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                Err(anyhow!("build_and_sign panicked: {}", panic_msg))
            }
        };
        let _ = build_tx.send(send_result);
    });

    let signed_core = match build_rx.recv_timeout(build_timeout) {
        Ok(Ok(core)) => core,
        Ok(Err(e)) => {
            let err_text = format!("{}", e);
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_sign_tx_error","timestamp":{},"location":"api.rs:2166","message":"build_and_sign failed","data":{{"wallet_id":"{}","error":{:?}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                    ts, wallet_id_for_log, &err_text
                );
            });
            return Err(anyhow!("Build/sign failed: {}", e));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_sign_tx_error","timestamp":{},"location":"api.rs:2166","message":"build_and_sign timeout","data":{{"wallet_id":"{}","timeout_secs":120}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                    ts, wallet_id_for_log
                );
            });
            return Err(anyhow!("Build/sign timed out after 120s"));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_sign_tx_error","timestamp":{},"location":"api.rs:2166","message":"build_and_sign failed","data":{{"wallet_id":"{}","error":"channel disconnected"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                    ts, wallet_id_for_log
                );
            });
            return Err(anyhow!("Build/sign failed: channel disconnected"));
        }
    };

    tracing::info!(
        "Signed transaction {}: {} bytes",
        signed_core.txid,
        signed_core.size
    );

    let tx_sapling_anchor_hex =
        encode_hex_opt(extract_sapling_anchor_from_raw_tx(&signed_core.raw_tx));
    let tx_orchard_anchor_hex =
        encode_hex_opt(extract_orchard_anchor_from_raw_tx(&signed_core.raw_tx));
    log_step(
        "tx_anchors",
        &format!(
            "target_height={},anchor_height={},tx_sapling_anchor={},tx_orchard_anchor={}",
            target_height, anchor_height, tx_sapling_anchor_hex, tx_orchard_anchor_hex
        ),
    );

    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_sign_tx_ok","timestamp":{},"location":"api.rs:2222","message":"sign_tx ok","data":{{"wallet_id":"{}","txid":"{}","size":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
            ts, wallet_id, signed_core.txid, signed_core.size
        );
    });

    if let Some(outgoing_memo_text) = pending
        .outputs
        .iter()
        .filter_map(|o| o.memo.as_ref())
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
    {
        let txid_hex = signed_core.txid.to_string();
        let memo_bytes =
            pirate_core::memo::Memo::from_text_truncated(outgoing_memo_text.to_string()).encode();
        if let Err(e) = repo.upsert_tx_memo(&txid_hex, &memo_bytes) {
            tracing::warn!("Failed to persist outgoing tx memo for {}: {}", txid_hex, e);
        }
    }

    clear_pending_sign_context(&pending.id);

    let spent_nullifiers: Vec<Vec<u8>> = signed_core
        .spent_notes
        .iter()
        .filter_map(|n| n.nullifier.clone())
        .collect();
    if !spent_nullifiers.is_empty() {
        store_broadcast_context(
            &signed_core.txid.to_string(),
            BroadcastContext {
                wallet_id: wallet_id.clone(),
                account_id: secret.account_id,
                spent_nullifiers,
                change_amount: pending.change,
                created_at_ms: unix_timestamp_millis(),
            },
        );
    }

    Ok(SignedTx {
        txid: signed_core.txid.to_string(),
        raw: signed_core.raw_tx,
        size: signed_core.size,
    })
}

pub(super) fn sign_tx(wallet_id: WalletId, pending: PendingTx) -> Result<SignedTx> {
    sign_tx_internal(wallet_id, pending, None, None)
}

pub(super) fn sign_tx_for_key(
    wallet_id: WalletId,
    pending: PendingTx,
    key_id: i64,
) -> Result<SignedTx> {
    sign_tx_internal(wallet_id, pending, Some(vec![key_id]), None)
}

pub(super) fn sign_tx_filtered(
    wallet_id: WalletId,
    pending: PendingTx,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<SignedTx> {
    sign_tx_internal(wallet_id, pending, key_ids_filter, address_ids_filter)
}

pub(super) async fn broadcast_tx(signed: SignedTx) -> Result<TxId> {
    tracing::info!("Broadcasting transaction {}", signed.txid);
    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_broadcast_start","timestamp":{},"location":"api.rs:2233","message":"broadcast start","data":{{"txid":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
            ts, signed.txid
        );
    });

    let wallet_id = get_active_wallet()?.ok_or_else(|| anyhow!("No active wallet"))?;

    let endpoint_config = get_lightd_endpoint_config(wallet_id.clone())?;
    let endpoint_url = endpoint_config.url();
    let client_config = tunnel::light_client_config_for_endpoint(
        &endpoint_config,
        RetryConfig::default(),
        std::time::Duration::from_secs(30),
        std::time::Duration::from_secs(60),
    );

    let client_config_for_retry = client_config.clone();
    let client = pirate_sync_lightd::LightClient::with_config(client_config);
    if let Err(e) = client.connect().await {
        let err_text = format!("{}", e);
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_broadcast_connect_error","timestamp":{},"location":"api.rs:2212","message":"broadcast connect failed","data":{{"endpoint":"{}","error":{:?}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                ts, endpoint_url, &err_text
            );
        });
        return Err(anyhow!(
            "Failed to connect to {}: {}",
            endpoint_url,
            err_text
        ));
    }

    let txid_hex = match client.broadcast(signed.raw.clone()).await {
        Ok(txid_hex) => txid_hex,
        Err(e) => {
            let err_text = format!("{}", e);
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_broadcast_error","timestamp":{},"location":"api.rs:2226","message":"broadcast failed","data":{{"txid":"{}","endpoint":"{}","error":{:?}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                    ts, signed.txid, endpoint_url, &err_text
                );
            });

            let err_lower = err_text.to_ascii_lowercase();

            if err_lower.contains("unknown-anchor") {
                let tx_orchard_anchor = extract_orchard_anchor_from_raw_tx(&signed.raw);
                let tx_sapling_anchor = extract_sapling_anchor_from_raw_tx(&signed.raw);
                let mut repair_from = 1u64;
                let mut repair_to_exclusive = 0u64;
                let mut local_orchard_anchor: Option<[u8; 32]> = None;
                let mut local_sapling_root: Option<[u8; 32]> = None;

                if let Ok((db, repo)) = open_wallet_db_for(&wallet_id) {
                    let spendability_storage = SpendabilityStateStorage::new(db);
                    let state = spendability_storage.load_state().unwrap_or_default();
                    repair_from = state.anchor_height.max(1);
                    repair_to_exclusive = state.target_height.max(repair_from).saturating_add(1);
                    local_orchard_anchor = repo
                        .resolve_orchard_anchor_from_db_state(repair_from)
                        .ok()
                        .flatten()
                        .map(|anchor| anchor.to_bytes());
                    local_sapling_root = repo
                        .resolve_sapling_root_from_db_state(repair_from)
                        .ok()
                        .flatten();
                }

                let tree_state_at_anchor = client.get_tree_state(repair_from).await.ok();
                let remote_sapling_root = tree_state_at_anchor
                    .as_ref()
                    .and_then(parse_sapling_root_from_tree_state);
                let remote_orchard_root = tree_state_at_anchor
                    .as_ref()
                    .and_then(parse_orchard_root_from_tree_state);
                let remote_bridge_orchard_root = client
                    .get_bridge_tree_state(repair_from)
                    .await
                    .ok()
                    .as_ref()
                    .and_then(parse_orchard_root_from_tree_state);

                pirate_core::debug_log::with_locked_file(|file| {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_unknown_anchor_diag","timestamp":{},"location":"api.rs:broadcast_tx","message":"unknown-anchor diagnostics","data":{{"wallet_id":"{}","txid":"{}","anchor_height":{},"tx_sapling_anchor":"{}","local_sapling_root":"{}","remote_sapling_root":"{}","tx_orchard_anchor":"{}","local_orchard_anchor":"{}","remote_orchard_root":"{}","remote_bridge_orchard_root":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        ts,
                        wallet_id,
                        signed.txid,
                        repair_from,
                        encode_hex_opt(tx_sapling_anchor),
                        encode_hex_opt(local_sapling_root),
                        encode_hex_opt(remote_sapling_root),
                        encode_hex_opt(tx_orchard_anchor),
                        encode_hex_opt(local_orchard_anchor),
                        encode_hex_opt(remote_orchard_root),
                        encode_hex_opt(remote_bridge_orchard_root),
                    );
                });

                let sapling_matches = tx_sapling_anchor.is_some()
                    && remote_sapling_root.is_some()
                    && tx_sapling_anchor == remote_sapling_root;
                let orchard_remote = remote_bridge_orchard_root.or(remote_orchard_root);
                let orchard_matches = tx_orchard_anchor.is_some()
                    && orchard_remote.is_some()
                    && tx_orchard_anchor == orchard_remote;
                let anchor_matches_remote = sapling_matches || orchard_matches;
                if anchor_matches_remote {
                    let retry_client =
                        pirate_sync_lightd::LightClient::with_config(client_config_for_retry);
                    if retry_client.connect().await.is_ok()
                        && retry_client.broadcast(signed.raw.clone()).await.is_ok()
                    {
                        pirate_core::debug_log::with_locked_file(|file| {
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis();
                            let _ = writeln!(
                                file,
                                r#"{{"id":"log_unknown_anchor_retry_ok","timestamp":{},"location":"api.rs:broadcast_tx","message":"unknown-anchor retry broadcast ok","data":{{"wallet_id":"{}","txid":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                                ts, wallet_id, signed.txid
                            );
                        });
                        return Ok(signed.txid);
                    }
                }

                if let Ok((db, _repo)) = open_wallet_db_for(&wallet_id) {
                    let spendability_storage = SpendabilityStateStorage::new(db);
                    if let Err(queue_err) = spendability_storage.queue_repair_range(
                        repair_from,
                        repair_to_exclusive.max(repair_from.saturating_add(1)),
                        SPENDABILITY_REASON_ERR_WITNESS_REPAIR_QUEUED,
                    ) {
                        tracing::warn!(
                            "Failed to queue unknown-anchor repair for {} ({}..{}): {}",
                            wallet_id,
                            repair_from,
                            repair_to_exclusive,
                            queue_err
                        );
                    }
                }

                sync_control::maybe_trigger_compact_sync(wallet_id.clone());

                return Err(anyhow!(
                    "{}: Node rejected transaction with unknown anchor. Witness repair was queued; let sync finalize and retry.",
                    SPENDABILITY_REASON_ERR_WITNESS_REPAIR_QUEUED
                ));
            }

            return Err(anyhow!("Broadcast failed: {}", e));
        }
    };

    tracing::info!(
        "Broadcast to {} succeeded: {} ({} bytes)",
        endpoint_url,
        txid_hex,
        signed.size
    );
    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_broadcast_ok","timestamp":{},"location":"api.rs:2263","message":"broadcast ok","data":{{"txid":"{}","endpoint":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
            ts, signed.txid, endpoint_url
        );
    });

    if let Some(ctx) = take_broadcast_context(&signed.txid) {
        let txid_bytes: [u8; 32] = {
            let decoded = hex::decode(&signed.txid).unwrap_or_default();
            let mut arr = [0u8; 32];
            if decoded.len() == 32 {
                arr.copy_from_slice(&decoded);
            }
            arr
        };
        if let Ok((db, repo)) = open_wallet_db_for(&ctx.wallet_id) {
            let entries: Vec<([u8; 32], [u8; 32])> = ctx
                .spent_nullifiers
                .iter()
                .filter_map(|nf| {
                    let mut arr = [0u8; 32];
                    if nf.len() == 32 {
                        arr.copy_from_slice(nf);
                        Some((arr, txid_bytes))
                    } else {
                        None
                    }
                })
                .collect();
            let marked = repo
                .mark_notes_spent_by_nullifiers_with_txid(ctx.account_id, &entries)
                .unwrap_or(0);
            tracing::info!(
                "Post-broadcast: marked {} notes as spent for tx {}",
                marked,
                signed.txid
            );
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_broadcast_post_mark","timestamp":{},"location":"api.rs:broadcast_tx","message":"post-broadcast note marking","data":{{"txid":"{}","wallet":"{}","nullifiers_submitted":{},"notes_marked":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                    ts,
                    signed.txid,
                    ctx.wallet_id,
                    entries.len(),
                    marked
                );
            });

            add_pending_change(&ctx.wallet_id, &signed.txid, ctx.change_amount);

            let _ = db;
        }
    }

    Ok(signed.txid)
}
