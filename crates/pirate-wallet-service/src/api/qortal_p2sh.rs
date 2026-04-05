use super::*;
use crate::models::{Output, SignedTx};
use pirate_core::keys::{ExtendedSpendingKey, OrchardExtendedSpendingKey, PaymentAddress};
use pirate_core::{
    build_qortal_p2sh_funding_transaction, build_qortal_p2sh_redeem_transaction, Memo,
    QortalP2shFundingPlan, QortalP2shRedeemPlan, QortalRecipient,
};
use zcash_client_backend::address::RecipientAddress;
use zcash_primitives::transaction::Transaction;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QortalP2shSendRequest {
    pub input: String,
    pub output: Vec<Output>,
    pub script: String,
    pub fee: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QortalP2shRedeemRequest {
    pub input: String,
    pub output: Vec<Output>,
    pub fee: u64,
    pub script: String,
    pub txid: String,
    pub locktime: u64,
    pub secret: String,
    pub privkey: String,
}

type RedeemValidation = (Vec<u8>, [u8; 32], Vec<u8>, Vec<u8>);

fn decode_base58_field(name: &str, value: &str, allow_empty: bool) -> Result<Vec<u8>> {
    if allow_empty && value.is_empty() {
        return Ok(Vec::new());
    }
    bs58::decode(value)
        .into_vec()
        .map_err(|e| anyhow!("Invalid {} base58: {}", name, e))
}

fn ensure_non_empty(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(anyhow!("{} must not be empty", name));
    }
    Ok(())
}

fn ensure_non_empty_bytes(name: &str, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        return Err(anyhow!("{} decoded to empty bytes", name));
    }
    Ok(())
}

fn decode_base58_field_exact(name: &str, value: &str, expected_len: usize) -> Result<Vec<u8>> {
    let bytes = decode_base58_field(name, value, false)?;
    if bytes.len() != expected_len {
        return Err(anyhow!(
            "{} must decode to exactly {} bytes (got {})",
            name,
            expected_len,
            bytes.len()
        ));
    }
    Ok(bytes)
}

fn validate_script_bytes(name: &str, bytes: &[u8]) -> Result<()> {
    ensure_non_empty_bytes(name, bytes)?;
    if bytes.len() < 2 {
        return Err(anyhow!(
            "{} is too short to be a valid output script ({} bytes)",
            name,
            bytes.len()
        ));
    }
    Ok(())
}

fn validate_qortal_input_address(network_type: NetworkType, input: &str) -> Result<()> {
    ensure_non_empty("input", input)?;
    let params = pirate_core::transaction::PirateNetwork::new(network_type);
    let input_address = RecipientAddress::decode(&params, input)
        .ok_or_else(|| anyhow!("Invalid input address: {}", input))?;
    match input_address {
        RecipientAddress::Transparent(_) => Ok(()),
        RecipientAddress::Shielded(_)
        | RecipientAddress::Orchard(_)
        | RecipientAddress::Unified(_) => Err(anyhow!(
            "Input address must be a transparent address for Qortal P2SH commands"
        )),
    }
}

fn parse_qortal_recipients(
    network_type: NetworkType,
    outputs: &[Output],
) -> Result<Vec<QortalRecipient>> {
    if outputs.is_empty() {
        return Err(anyhow!("At least one output is required"));
    }
    outputs
        .iter()
        .enumerate()
        .map(|(index, output)| {
            if output.addr.trim().is_empty() {
                return Err(anyhow!("Output address must not be empty"));
            }
            if output.amount == 0 {
                return Err(anyhow!("Output amount must be greater than zero"));
            }
            parse_qortal_recipient(network_type, output, index)
        })
        .collect()
}

fn validate_send_request(
    network_type: NetworkType,
    request: &QortalP2shSendRequest,
) -> Result<Vec<u8>> {
    validate_qortal_input_address(network_type, &request.input)?;
    let script_pubkey = decode_base58_field("script", &request.script, false)?;
    validate_script_bytes("script", &script_pubkey)?;
    Ok(script_pubkey)
}

fn validate_redeem_request(
    network_type: NetworkType,
    request: &QortalP2shRedeemRequest,
) -> Result<RedeemValidation> {
    validate_qortal_input_address(network_type, &request.input)?;

    let redeem_script = decode_base58_field("script", &request.script, false)?;
    validate_script_bytes("script", &redeem_script)?;

    let txid_bytes = decode_base58_field_exact("txid", &request.txid, 32)?;
    let mut txid = [0u8; 32];
    txid.copy_from_slice(&txid_bytes);

    let secret = decode_base58_field("secret", &request.secret, true)?;
    if request.locktime == 0 && secret.is_empty() {
        return Err(anyhow!(
            "secret must be provided for redeem flow (locktime = 0)"
        ));
    }
    if request.locktime > 0 && !secret.is_empty() {
        return Err(anyhow!(
            "secret must be empty for refund flow (locktime > 0)"
        ));
    }

    let privkey_bytes = decode_base58_field_exact("privkey", &request.privkey, 32)?;

    Ok((redeem_script, txid, secret, privkey_bytes))
}

fn parse_qortal_recipient(
    network_type: NetworkType,
    output: &Output,
    output_index: usize,
) -> Result<QortalRecipient> {
    if output.amount == 0 {
        return Err(anyhow!("output #{} has zero amount", output_index));
    }

    let params = pirate_core::transaction::PirateNetwork::new(network_type);
    let memo = output
        .memo
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| Memo::from_text_truncated(s.clone()));

    let recipient = RecipientAddress::decode(&params, &output.addr).ok_or_else(|| {
        anyhow!(
            "Invalid recipient address in output #{}: {}",
            output_index,
            output.addr
        )
    })?;

    match recipient {
        RecipientAddress::Shielded(address) => Ok(QortalRecipient::Sapling {
            address: PaymentAddress { inner: address },
            amount: output.amount,
            memo,
        }),
        RecipientAddress::Orchard(address) => Ok(QortalRecipient::Orchard {
            address,
            amount: output.amount,
            memo,
        }),
        RecipientAddress::Transparent(address) => Ok(QortalRecipient::Transparent {
            address,
            amount: output.amount,
        }),
        RecipientAddress::Unified(_) => Err(anyhow!(
            "Unified addresses are not supported for Qortal P2SH commands"
        )),
    }
}

fn active_network_type(wallet_id: &WalletId) -> Result<NetworkType> {
    let wallet = get_wallet_meta(wallet_id)?;
    Ok(match wallet.network_type.as_deref().unwrap_or("mainnet") {
        "testnet" => NetworkType::Testnet,
        "regtest" => NetworkType::Regtest,
        _ => NetworkType::Mainnet,
    })
}

fn signed_tx_from_core(tx: pirate_core::shielded_builder::SignedShieldedTransaction) -> SignedTx {
    SignedTx {
        txid: tx.txid.to_string(),
        raw: tx.raw_tx,
        size: tx.size,
    }
}

fn current_orchard_anchor(repo: &Repository, height: u64) -> Result<Option<orchard::tree::Anchor>> {
    repo.resolve_orchard_anchor_from_db_state(height)
        .map_err(|e| anyhow!("Failed to resolve Orchard anchor: {}", e))
}

fn resolve_fixed_internal_change_index(
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

pub async fn qortal_send_p2sh(
    wallet_id: WalletId,
    request: QortalP2shSendRequest,
) -> Result<String> {
    let network_type = active_network_type(&wallet_id)?;
    let script_pubkey = validate_send_request(network_type, &request)?;

    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    let source_address = repo
        .get_address_by_string(secret.account_id, &request.input)?
        .ok_or_else(|| {
            anyhow!(
                "Input address {} is not owned by wallet {}",
                request.input,
                wallet_id
            )
        })?;
    let source_key_id = source_address
        .key_id
        .ok_or_else(|| anyhow!("Input address is missing key metadata"))?;

    let key = repo
        .get_account_key_by_id(source_key_id)?
        .ok_or_else(|| anyhow!("Key group not found"))?;
    if !key.spendable {
        return Err(anyhow!(
            "Input address belongs to a non-spendable key group"
        ));
    }

    let sapling_extsk_bytes = key
        .sapling_extsk
        .clone()
        .or_else(|| Some(secret.extsk.clone()))
        .ok_or_else(|| anyhow!("Sapling spending key missing"))?;
    let default_sapling_key = ExtendedSpendingKey::from_bytes(&sapling_extsk_bytes)
        .map_err(|e| anyhow!("Invalid Sapling spending key bytes: {}", e))?;

    let default_orchard_key = key
        .orchard_extsk
        .as_ref()
        .or(secret.orchard_extsk.as_ref())
        .map(|bytes| {
            OrchardExtendedSpendingKey::from_bytes(bytes)
                .map_err(|e| anyhow!("Invalid Orchard spending key bytes: {}", e))
        })
        .transpose()?;

    let mut sapling_keys_by_id = HashMap::new();
    let mut orchard_keys_by_id = HashMap::new();
    for account_key in repo.get_account_keys(secret.account_id)? {
        if !account_key.spendable {
            continue;
        }
        let Some(key_id) = account_key.id else {
            continue;
        };
        if let Some(bytes) = account_key.sapling_extsk.as_ref() {
            if let Ok(parsed) = ExtendedSpendingKey::from_bytes(bytes) {
                sapling_keys_by_id.insert(key_id, parsed);
            }
        }
        if let Some(bytes) = account_key.orchard_extsk.as_ref() {
            if let Ok(parsed) = OrchardExtendedSpendingKey::from_bytes(bytes) {
                orchard_keys_by_id.insert(key_id, parsed);
            }
        }
    }

    let notes = repo.get_unspent_selectable_notes_filtered(
        secret.account_id,
        None,
        Some(vec![source_address
            .id
            .ok_or_else(|| anyhow!("Input address row id missing"))?]),
    )?;
    if notes.is_empty() {
        return Err(anyhow!(
            "Input address {} has no spendable shielded notes",
            request.input
        ));
    }

    let spendability = sync_control::require_spendability_ready_with_sync_trigger(&wallet_id)?;
    let orchard_anchor = if notes
        .iter()
        .any(|note| note.note_type == pirate_core::selection::NoteType::Orchard)
        || request.output.iter().any(|output| {
            output.addr.starts_with("pirate1")
                || output.addr.starts_with("pirate-test1")
                || output.addr.starts_with("pirate-regtest1")
        }) {
        current_orchard_anchor(&repo, spendability.anchor_height)?
    } else {
        None
    };

    let recipients = parse_qortal_recipients(network_type, &request.output)?;

    let has_orchard_spends = notes
        .iter()
        .any(|note| note.note_type == pirate_core::selection::NoteType::Orchard);
    let has_orchard_outputs = request.output.iter().any(|output| {
        output.addr.starts_with("pirate1")
            || output.addr.starts_with("pirate-test1")
            || output.addr.starts_with("pirate-regtest1")
    });
    let use_orchard_change = has_orchard_spends || has_orchard_outputs;

    let change_index =
        resolve_fixed_internal_change_index(&repo, secret.account_id, source_key_id)?;
    let (change_addr, address_type) = if use_orchard_change {
        let orchard_key = default_orchard_key
            .as_ref()
            .ok_or_else(|| anyhow!("Orchard spending key required for Orchard change"))?;
        let address = orchard_key
            .to_extended_fvk()
            .address_at_internal(change_index)
            .encode_for_network(network_type)?;
        (address, pirate_storage_sqlite::AddressType::Orchard)
    } else {
        let address = default_sapling_key
            .to_internal_fvk()
            .derive_address(change_index)
            .encode_for_network(network_type);
        (address, pirate_storage_sqlite::AddressType::Sapling)
    };

    let change_address = pirate_storage_sqlite::Address {
        id: None,
        key_id: Some(source_key_id),
        account_id: secret.account_id,
        diversifier_index: change_index,
        address: change_addr,
        address_type,
        label: None,
        created_at: chrono::Utc::now().timestamp(),
        color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
        address_scope: pirate_storage_sqlite::AddressScope::Internal,
    };
    repo.upsert_address(&change_address)?;

    let signed = build_qortal_p2sh_funding_transaction(QortalP2shFundingPlan {
        network_type,
        default_sapling_spending_key: &default_sapling_key,
        default_orchard_spending_key: default_orchard_key.as_ref(),
        sapling_spending_keys_by_id: sapling_keys_by_id,
        orchard_spending_keys_by_id: orchard_keys_by_id,
        available_notes: notes,
        target_height: u32::try_from(spendability.target_height)
            .map_err(|_| anyhow!("Target height exceeds u32"))?,
        orchard_anchor,
        change_diversifier_index: change_index,
        recipients,
        fee: request.fee,
        script_pubkey,
        script_each_recipient: true,
    })?;

    let signed = signed_tx_from_core(signed);
    tx_flow::broadcast_tx(signed).await
}

pub async fn qortal_redeem_p2sh(
    wallet_id: WalletId,
    request: QortalP2shRedeemRequest,
) -> Result<String> {
    let network_type = active_network_type(&wallet_id)?;
    let (redeem_script, txid, secret, privkey_bytes) =
        validate_redeem_request(network_type, &request)?;

    let endpoint_config = get_lightd_endpoint_config(wallet_id.clone())?;
    let client_config = tunnel::light_client_config_for_endpoint(
        &endpoint_config,
        RetryConfig::default(),
        std::time::Duration::from_secs(30),
        std::time::Duration::from_secs(60),
    );
    let client = pirate_sync_lightd::LightClient::with_config(client_config);
    client.connect().await?;

    let latest_height = client.get_latest_block().await?;
    let target_height =
        u32::try_from(latest_height).map_err(|_| anyhow!("Latest block height exceeds u32"))?;

    let funding_raw = client.get_transaction(&txid).await?;
    let funding_tx = Transaction::read(&funding_raw[..], BranchId::Nu5)
        .or_else(|_| Transaction::read(&funding_raw[..], BranchId::Canopy))
        .map_err(|e| anyhow!("Failed to parse funding transaction: {}", e))?;
    let funding_output = funding_tx
        .transparent_bundle()
        .and_then(|bundle| bundle.vout.first())
        .cloned()
        .ok_or_else(|| anyhow!("Funding transaction is missing transparent output 0"))?;

    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let orchard_anchor = if request.output.iter().any(|output| {
        output.addr.starts_with("pirate1")
            || output.addr.starts_with("pirate-test1")
            || output.addr.starts_with("pirate-regtest1")
    }) {
        let spendability = sync_control::require_spendability_ready_with_sync_trigger(&wallet_id)?;
        current_orchard_anchor(&repo, spendability.anchor_height)?
    } else {
        None
    };

    let recipients = parse_qortal_recipients(network_type, &request.output)?;

    let privkey = secp256k1::SecretKey::from_slice(&privkey_bytes)
        .map_err(|e| anyhow!("Invalid privkey: {}", e))?;
    let lock_time =
        u32::try_from(request.locktime).map_err(|_| anyhow!("locktime out of range"))?;

    let signed = build_qortal_p2sh_redeem_transaction(QortalP2shRedeemPlan {
        network_type,
        target_height,
        orchard_anchor,
        funding_txid: txid,
        funding_coin: funding_output,
        recipients,
        fee: request.fee,
        redeem_script,
        lock_time,
        secret,
        privkey,
    })?;

    let signed = SignedTx {
        txid: signed.txid.to_string(),
        raw: signed.raw_tx,
        size: signed.size,
    };
    tx_flow::broadcast_tx(signed).await
}

#[cfg(test)]
mod tests {
    use super::{
        decode_base58_field, decode_base58_field_exact, ensure_non_empty, parse_qortal_recipients,
        validate_redeem_request, validate_script_bytes, validate_send_request,
        QortalP2shRedeemRequest, QortalP2shSendRequest,
    };
    use crate::api::NetworkType;
    use crate::models::Output;
    use pirate_core::keys::OrchardExtendedSpendingKey;
    use pirate_core::transaction::PirateNetwork;
    use zcash_client_backend::address::RecipientAddress;
    use zcash_primitives::legacy::TransparentAddress;

    fn sample_transparent_input() -> String {
        RecipientAddress::Transparent(TransparentAddress::PublicKey([7u8; 20]))
            .encode(&PirateNetwork::new(NetworkType::Mainnet))
    }

    fn sample_output() -> Output {
        let orchard_key = OrchardExtendedSpendingKey::master(&[7u8; 32]).unwrap();
        let orchard_address = orchard_key
            .to_extended_fvk()
            .address_at(0)
            .encode_for_network(NetworkType::Mainnet)
            .unwrap();
        Output::new(orchard_address, 10_000, Some("memo".to_string()))
    }

    #[test]
    fn send_request_rejects_empty_input() {
        let request = QortalP2shSendRequest {
            input: String::new(),
            output: vec![sample_output()],
            script: bs58::encode(vec![1u8; 4]).into_string(),
            fee: 10_000,
        };
        let err = validate_send_request(NetworkType::Mainnet, &request).unwrap_err();
        assert!(err.to_string().contains("input"));
    }

    #[test]
    fn outputs_must_not_be_empty() {
        let err = parse_qortal_recipients(NetworkType::Mainnet, &[]).unwrap_err();
        assert!(err.to_string().contains("At least one output"));
    }

    #[test]
    fn outputs_must_not_have_zero_amount() {
        let outputs = vec![Output::new(sample_output().addr, 0, None)];
        let err = parse_qortal_recipients(NetworkType::Mainnet, &outputs).unwrap_err();
        assert!(err.to_string().contains("greater than zero"));
    }

    #[test]
    fn ensure_non_empty_rejects_blank_strings() {
        let err = ensure_non_empty("script", "   ").unwrap_err();
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn decode_base58_field_exact_rejects_wrong_length() {
        let encoded = bs58::encode(vec![1u8; 31]).into_string();
        let err = decode_base58_field_exact("privkey", &encoded, 32).unwrap_err();
        assert!(err.to_string().contains("must decode to exactly 32 bytes"));
    }

    #[test]
    fn validate_script_bytes_rejects_short_script() {
        let err = validate_script_bytes("script", &[1u8]).unwrap_err();
        assert!(err.to_string().contains("too short"));
    }

    #[test]
    fn redeem_requires_secret_when_locktime_zero() {
        let request = QortalP2shRedeemRequest {
            input: sample_transparent_input(),
            output: vec![sample_output()],
            fee: 10_000,
            script: bs58::encode(vec![1u8; 4]).into_string(),
            txid: bs58::encode(vec![2u8; 32]).into_string(),
            locktime: 0,
            secret: String::new(),
            privkey: bs58::encode(vec![3u8; 32]).into_string(),
        };
        let err = validate_redeem_request(NetworkType::Mainnet, &request).unwrap_err();
        assert!(err.to_string().contains("secret must be provided"));
    }

    #[test]
    fn refund_requires_empty_secret_when_locktime_non_zero() {
        let request = QortalP2shRedeemRequest {
            input: sample_transparent_input(),
            output: vec![sample_output()],
            fee: 10_000,
            script: bs58::encode(vec![1u8; 4]).into_string(),
            txid: bs58::encode(vec![2u8; 32]).into_string(),
            locktime: 5,
            secret: bs58::encode(vec![4u8; 32]).into_string(),
            privkey: bs58::encode(vec![3u8; 32]).into_string(),
        };
        let err = validate_redeem_request(NetworkType::Mainnet, &request).unwrap_err();
        assert!(err.to_string().contains("secret must be empty"));
    }

    #[test]
    fn refund_request_accepts_empty_secret() {
        let request = QortalP2shRedeemRequest {
            input: sample_transparent_input(),
            output: vec![sample_output()],
            fee: 10_000,
            script: bs58::encode(vec![1u8; 4]).into_string(),
            txid: bs58::encode(vec![2u8; 32]).into_string(),
            locktime: 5,
            secret: String::new(),
            privkey: bs58::encode(vec![3u8; 32]).into_string(),
        };
        validate_redeem_request(NetworkType::Mainnet, &request).unwrap();
    }

    #[test]
    fn redeem_requires_transparent_input_address() {
        let request = QortalP2shRedeemRequest {
            input: sample_output().addr,
            output: vec![sample_output()],
            fee: 10_000,
            script: bs58::encode(vec![1u8; 4]).into_string(),
            txid: bs58::encode(vec![2u8; 32]).into_string(),
            locktime: 5,
            secret: String::new(),
            privkey: bs58::encode(vec![3u8; 32]).into_string(),
        };
        let err = validate_redeem_request(NetworkType::Mainnet, &request).unwrap_err();
        assert!(err.to_string().contains("transparent address"));
    }

    #[test]
    fn send_request_shape_accepts_minimal_valid_values() {
        let request = QortalP2shSendRequest {
            input: sample_transparent_input(),
            output: vec![sample_output()],
            script: bs58::encode(vec![1u8; 4]).into_string(),
            fee: 10_000,
        };
        let recipients = parse_qortal_recipients(NetworkType::Mainnet, &request.output).unwrap();
        assert_eq!(recipients.len(), 1);
        validate_send_request(NetworkType::Mainnet, &request).unwrap();
        let script = decode_base58_field("script", &request.script, false).unwrap();
        assert_eq!(script, vec![1u8; 4]);
    }

    #[test]
    fn orchard_recipients_are_accepted_when_valid() {
        let orchard_key = OrchardExtendedSpendingKey::master(&[7u8; 32]).unwrap();
        let orchard_address = orchard_key
            .to_extended_fvk()
            .address_at(0)
            .encode_for_network(NetworkType::Mainnet)
            .unwrap();
        let outputs = vec![Output::new(
            orchard_address,
            10_000,
            Some("memo".to_string()),
        )];

        let recipients = parse_qortal_recipients(NetworkType::Mainnet, &outputs).unwrap();
        assert_eq!(recipients.len(), 1);
        assert!(matches!(
            recipients[0],
            pirate_core::QortalRecipient::Orchard { .. }
        ));
    }
}
