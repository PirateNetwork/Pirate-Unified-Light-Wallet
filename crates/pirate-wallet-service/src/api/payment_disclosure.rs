use super::*;
use bech32::{Bech32, Hrp};
use std::convert::TryInto;
use zcash_note_encryption::{try_output_recovery_with_ock, Domain, EphemeralKeyBytes};
use zcash_primitives::sapling::note_encryption::{prf_ock, try_sapling_output_recovery_with_ock};
use zcash_primitives::transaction::TxId as ZcashTxId;

const SAPLING_DISCLOSURE_MAINNET_HRP: &str = "pirate-sapling-payment-disclosure";
const ORCHARD_DISCLOSURE_MAINNET_HRP: &str = "pirate-orchard-payment-disclosure";
const SAPLING_DISCLOSURE_TESTNET_HRP: &str = "zdisctest";
const ORCHARD_DISCLOSURE_TESTNET_HRP: &str = "odisctest";
const SAPLING_DISCLOSURE_REGTEST_HRP: &str = "zdiscregtest";
const ORCHARD_DISCLOSURE_REGTEST_HRP: &str = "odiscregtest";
const DISCLOSURE_PAYLOAD_LEN: usize = 32 + 4 + 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisclosureKind {
    Sapling,
    Orchard,
}

impl DisclosureKind {
    fn as_str(self) -> &'static str {
        match self {
            DisclosureKind::Sapling => "sapling",
            DisclosureKind::Orchard => "orchard",
        }
    }
}

#[derive(Debug, Clone)]
struct DecodedDisclosure {
    kind: DisclosureKind,
    network_type: NetworkType,
    txid_bytes: [u8; 32],
    output_index: u32,
    ock: [u8; 32],
}

#[derive(Debug, Clone)]
struct RawTransactionFetch {
    bytes: Vec<u8>,
    height: Option<u32>,
}

fn sapling_disclosure_hrp(network_type: NetworkType) -> &'static str {
    match network_type {
        NetworkType::Mainnet => SAPLING_DISCLOSURE_MAINNET_HRP,
        NetworkType::Testnet => SAPLING_DISCLOSURE_TESTNET_HRP,
        NetworkType::Regtest => SAPLING_DISCLOSURE_REGTEST_HRP,
    }
}

fn orchard_disclosure_hrp(network_type: NetworkType) -> &'static str {
    match network_type {
        NetworkType::Mainnet => ORCHARD_DISCLOSURE_MAINNET_HRP,
        NetworkType::Testnet => ORCHARD_DISCLOSURE_TESTNET_HRP,
        NetworkType::Regtest => ORCHARD_DISCLOSURE_REGTEST_HRP,
    }
}

fn disclosure_hrp(kind: DisclosureKind, network_type: NetworkType) -> &'static str {
    match kind {
        DisclosureKind::Sapling => sapling_disclosure_hrp(network_type),
        DisclosureKind::Orchard => orchard_disclosure_hrp(network_type),
    }
}

fn disclosure_kind_for_hrp(hrp: &str) -> Option<(DisclosureKind, NetworkType)> {
    match hrp {
        SAPLING_DISCLOSURE_MAINNET_HRP => Some((DisclosureKind::Sapling, NetworkType::Mainnet)),
        ORCHARD_DISCLOSURE_MAINNET_HRP => Some((DisclosureKind::Orchard, NetworkType::Mainnet)),
        SAPLING_DISCLOSURE_TESTNET_HRP => Some((DisclosureKind::Sapling, NetworkType::Testnet)),
        ORCHARD_DISCLOSURE_TESTNET_HRP => Some((DisclosureKind::Orchard, NetworkType::Testnet)),
        SAPLING_DISCLOSURE_REGTEST_HRP => Some((DisclosureKind::Sapling, NetworkType::Regtest)),
        ORCHARD_DISCLOSURE_REGTEST_HRP => Some((DisclosureKind::Orchard, NetworkType::Regtest)),
        _ => None,
    }
}

fn encode_payment_disclosure(
    kind: DisclosureKind,
    network_type: NetworkType,
    txid_bytes: &[u8; 32],
    output_index: u32,
    ock: &[u8; 32],
) -> Result<String> {
    let mut payload = Vec::with_capacity(DISCLOSURE_PAYLOAD_LEN);
    payload.extend_from_slice(txid_bytes);
    payload.extend_from_slice(&output_index.to_le_bytes());
    payload.extend_from_slice(ock);

    let hrp = Hrp::parse(disclosure_hrp(kind, network_type))
        .map_err(|e| anyhow!("Invalid payment disclosure HRP: {}", e))?;
    bech32::encode::<Bech32>(hrp, &payload)
        .map_err(|e| anyhow!("Failed to encode payment disclosure: {}", e))
}

fn decode_payment_disclosure(disclosure: &str) -> Result<DecodedDisclosure> {
    let normalized = disclosure
        .trim()
        .strip_prefix("zpd:")
        .unwrap_or_else(|| disclosure.trim());
    let (hrp, payload) = bech32::decode(normalized)
        .map_err(|e| anyhow!("Invalid payment disclosure encoding: {}", e))?;
    let (kind, network_type) = disclosure_kind_for_hrp(&hrp.to_string())
        .ok_or_else(|| anyhow!("Unsupported payment disclosure prefix: {}", hrp))?;

    if payload.len() != DISCLOSURE_PAYLOAD_LEN {
        return Err(anyhow!(
            "Invalid payment disclosure payload length: {} (expected {})",
            payload.len(),
            DISCLOSURE_PAYLOAD_LEN
        ));
    }

    let txid_bytes: [u8; 32] = payload[0..32]
        .try_into()
        .map_err(|_| anyhow!("Invalid transaction id bytes in disclosure"))?;
    let output_index = u32::from_le_bytes(
        payload[32..36]
            .try_into()
            .map_err(|_| anyhow!("Invalid output index bytes in disclosure"))?,
    );
    let ock: [u8; 32] = payload[36..68]
        .try_into()
        .map_err(|_| anyhow!("Invalid OCK bytes in disclosure"))?;

    Ok(DecodedDisclosure {
        kind,
        network_type,
        txid_bytes,
        output_index,
        ock,
    })
}

fn memo_to_text(memo: &[u8]) -> Option<String> {
    if memo.iter().all(|b| *b == 0) {
        None
    } else {
        pirate_sync_lightd::sapling::full_decrypt::decode_memo(memo)
    }
}

fn txid_string(txid_bytes: &[u8; 32]) -> String {
    ZcashTxId::from_bytes(*txid_bytes).to_string()
}

fn push_unique_ovk(bytes: [u8; 32], seen: &mut HashSet<[u8; 32]>, out: &mut Vec<[u8; 32]>) {
    if seen.insert(bytes) {
        out.push(bytes);
    }
}

fn ovk_candidate_bytes(
    sapling_ovks: &[SaplingOutgoingViewingKey],
    orchard_ovks: &[orchard::keys::OutgoingViewingKey],
    primary: DisclosureKind,
) -> Vec<[u8; 32]> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

    let push_sapling = |seen: &mut HashSet<[u8; 32]>, candidates: &mut Vec<[u8; 32]>| {
        for ovk in sapling_ovks {
            push_unique_ovk(ovk.0, seen, candidates);
        }
    };
    let push_orchard = |seen: &mut HashSet<[u8; 32]>, candidates: &mut Vec<[u8; 32]>| {
        for ovk in orchard_ovks {
            push_unique_ovk(*ovk.as_ref(), seen, candidates);
        }
    };

    match primary {
        DisclosureKind::Sapling => {
            push_sapling(&mut seen, &mut candidates);
            push_orchard(&mut seen, &mut candidates);
        }
        DisclosureKind::Orchard => {
            push_orchard(&mut seen, &mut candidates);
            push_sapling(&mut seen, &mut candidates);
        }
    }

    candidates
}

fn recover_payment_disclosures_from_raw_tx(
    raw_tx_bytes: &[u8],
    tx_height: Option<u32>,
    sapling_ovks: &[SaplingOutgoingViewingKey],
    orchard_ovks: &[orchard::keys::OutgoingViewingKey],
    network_type: NetworkType,
) -> Vec<PaymentDisclosure> {
    let tx = match Transaction::read(raw_tx_bytes, BranchId::Nu5)
        .or_else(|_| Transaction::read(raw_tx_bytes, BranchId::Canopy))
    {
        Ok(tx) => tx,
        Err(_) => return Vec::new(),
    };
    let txid_bytes = *tx.txid().as_ref();
    recover_payment_disclosures_from_tx(
        &tx,
        &txid_bytes,
        tx_height,
        sapling_ovks,
        orchard_ovks,
        network_type,
    )
}

fn recover_payment_disclosures_from_tx(
    tx: &Transaction,
    txid_bytes: &[u8; 32],
    tx_height: Option<u32>,
    sapling_ovks: &[SaplingOutgoingViewingKey],
    orchard_ovks: &[orchard::keys::OutgoingViewingKey],
    network_type: NetworkType,
) -> Vec<PaymentDisclosure> {
    let mut disclosures = Vec::new();
    if sapling_ovks.is_empty() && orchard_ovks.is_empty() {
        return disclosures;
    }

    let network = PirateNetwork::new(network_type);
    let block_height = BlockHeight::from_u32(tx_height.unwrap_or(0));
    let txid = txid_string(txid_bytes);

    if let Some(bundle) = tx.sapling_bundle() {
        let candidates = ovk_candidate_bytes(sapling_ovks, orchard_ovks, DisclosureKind::Sapling);
        for (idx, output) in bundle.shielded_outputs().iter().enumerate() {
            for ovk_bytes in &candidates {
                let ovk = SaplingOutgoingViewingKey(*ovk_bytes);
                let cmu_bytes = output.cmu().to_bytes();
                let ock = prf_ock(&ovk, output.cv(), &cmu_bytes, output.ephemeral_key());
                if let Some((note, address, memo)) =
                    try_sapling_output_recovery_with_ock(&network, block_height, &ock, output)
                {
                    let ock_bytes = ock.0;
                    let disclosure = match encode_payment_disclosure(
                        DisclosureKind::Sapling,
                        network_type,
                        txid_bytes,
                        idx as u32,
                        &ock_bytes,
                    ) {
                        Ok(value) => value,
                        Err(_) => break,
                    };
                    let memo_vec = memo.as_array().to_vec();
                    disclosures.push(PaymentDisclosure {
                        disclosure_type: DisclosureKind::Sapling.as_str().to_string(),
                        txid: txid.clone(),
                        output_index: idx as u32,
                        address: PaymentAddress { inner: address }.encode_for_network(network_type),
                        amount: note.value().inner(),
                        memo: memo_to_text(&memo_vec),
                        disclosure,
                    });
                    break;
                }
            }
        }
    }

    if let Some(bundle) = tx.orchard_bundle() {
        let candidates = ovk_candidate_bytes(sapling_ovks, orchard_ovks, DisclosureKind::Orchard);
        for (idx, action) in bundle.actions().iter().enumerate() {
            for ovk_bytes in &candidates {
                let ovk = orchard::keys::OutgoingViewingKey::from(*ovk_bytes);
                let epk = EphemeralKeyBytes(action.encrypted_note().epk_bytes);
                let cmx_bytes = action.cmx().to_bytes();
                let ock =
                    <OrchardDomain as Domain>::derive_ock(&ovk, action.cv_net(), &cmx_bytes, &epk);
                let domain = OrchardDomain::for_action(action);
                if let Some((note, address, memo)) = try_output_recovery_with_ock(
                    &domain,
                    &ock,
                    action,
                    &action.encrypted_note().out_ciphertext,
                ) {
                    let ock_bytes = ock.0;
                    let disclosure = match encode_payment_disclosure(
                        DisclosureKind::Orchard,
                        network_type,
                        txid_bytes,
                        idx as u32,
                        &ock_bytes,
                    ) {
                        Ok(value) => value,
                        Err(_) => break,
                    };
                    let memo_vec = memo.to_vec();
                    let address_string = match (OrchardPaymentAddress { inner: address })
                        .encode_for_network(network_type)
                    {
                        Ok(address) => address,
                        Err(_) => continue,
                    };
                    disclosures.push(PaymentDisclosure {
                        disclosure_type: DisclosureKind::Orchard.as_str().to_string(),
                        txid: txid.clone(),
                        output_index: idx as u32,
                        address: address_string,
                        amount: note.value().inner(),
                        memo: memo_to_text(&memo_vec),
                        disclosure,
                    });
                    break;
                }
            }
        }
    }

    disclosures
}

pub(super) fn recover_outgoing_recipients_with_disclosures_from_raw_tx(
    raw_tx_bytes: &[u8],
    tx_height: Option<u32>,
    sapling_ovks: &[SaplingOutgoingViewingKey],
    orchard_ovks: &[orchard::keys::OutgoingViewingKey],
    network_type: NetworkType,
) -> Vec<TransactionRecipient> {
    recover_payment_disclosures_from_raw_tx(
        raw_tx_bytes,
        tx_height,
        sapling_ovks,
        orchard_ovks,
        network_type,
    )
    .into_iter()
    .map(|disclosure| TransactionRecipient {
        address: disclosure.address,
        pool: disclosure.disclosure_type,
        amount: disclosure.amount,
        output_index: disclosure.output_index,
        memo: disclosure.memo,
        payment_disclosure: Some(disclosure.disclosure),
    })
    .collect()
}

async fn fetch_raw_transaction(
    endpoint_config: endpoint::LightdEndpoint,
    tx_hash_candidates: Vec<[u8; 32]>,
) -> Result<RawTransactionFetch> {
    let client_config = tunnel::light_client_config_for_endpoint(
        &endpoint_config,
        RetryConfig::default(),
        Duration::from_secs(30),
        Duration::from_secs(60),
    );
    let client = LightClient::with_config(client_config);
    client
        .connect()
        .await
        .map_err(|e| anyhow!("Failed to connect to lightwalletd: {}", e))?;

    let mut last_fetch_err: Option<String> = None;
    for tx_hash in tx_hash_candidates {
        match client.get_raw_transaction(&tx_hash).await {
            Ok(raw) => {
                let height = raw.height.and_then(|height| u32::try_from(height).ok());
                return Ok(RawTransactionFetch {
                    bytes: raw.data,
                    height,
                });
            }
            Err(e) => last_fetch_err = Some(e.to_string()),
        }
    }

    Err(anyhow!(
        "Failed to fetch raw transaction: {}",
        last_fetch_err.unwrap_or_else(|| "unknown error".to_string())
    ))
}

/// Export all payment disclosures recoverable by this wallet for an outgoing transaction.
pub async fn export_payment_disclosures(
    wallet_id: WalletId,
    txid: String,
) -> Result<Vec<PaymentDisclosure>> {
    run_on_runtime(move || export_payment_disclosures_inner(wallet_id, txid)).await
}

async fn export_payment_disclosures_inner(
    wallet_id: WalletId,
    txid: String,
) -> Result<Vec<PaymentDisclosure>> {
    let (endpoint_config, tx_hash_candidates, sapling_ovks, orchard_ovks, tx_height_hint) =
        collect_tx_recovery_context(&wallet_id, &txid)?;
    let raw = fetch_raw_transaction(endpoint_config, tx_hash_candidates).await?;
    let network_type = address_prefix_network_type(&wallet_id)?;

    Ok(recover_payment_disclosures_from_raw_tx(
        &raw.bytes,
        raw.height.or(tx_height_hint),
        &sapling_ovks,
        &orchard_ovks,
        network_type,
    ))
}

/// Export a Sapling payment disclosure for a specific output index.
pub async fn export_sapling_payment_disclosure(
    wallet_id: WalletId,
    txid: String,
    output_index: u32,
) -> Result<String> {
    run_on_runtime(move || export_sapling_payment_disclosure_inner(wallet_id, txid, output_index))
        .await
}

async fn export_sapling_payment_disclosure_inner(
    wallet_id: WalletId,
    txid: String,
    output_index: u32,
) -> Result<String> {
    export_payment_disclosures_inner(wallet_id, txid)
        .await?
        .into_iter()
        .find(|d| {
            d.disclosure_type == DisclosureKind::Sapling.as_str() && d.output_index == output_index
        })
        .map(|d| d.disclosure)
        .ok_or_else(|| {
            anyhow!(
                "No Sapling payment disclosure found for output index {}",
                output_index
            )
        })
}

/// Export an Orchard payment disclosure for a specific action index.
pub async fn export_orchard_payment_disclosure(
    wallet_id: WalletId,
    txid: String,
    action_index: u32,
) -> Result<String> {
    run_on_runtime(move || export_orchard_payment_disclosure_inner(wallet_id, txid, action_index))
        .await
}

async fn export_orchard_payment_disclosure_inner(
    wallet_id: WalletId,
    txid: String,
    action_index: u32,
) -> Result<String> {
    export_payment_disclosures_inner(wallet_id, txid)
        .await?
        .into_iter()
        .find(|d| {
            d.disclosure_type == DisclosureKind::Orchard.as_str() && d.output_index == action_index
        })
        .map(|d| d.disclosure)
        .ok_or_else(|| {
            anyhow!(
                "No Orchard payment disclosure found for action index {}",
                action_index
            )
        })
}

/// Verify and decrypt a Sapling or Orchard payment disclosure.
pub async fn verify_payment_disclosure(
    wallet_id: WalletId,
    disclosure: String,
) -> Result<PaymentDisclosureVerification> {
    run_on_runtime(move || verify_payment_disclosure_inner(wallet_id, disclosure)).await
}

async fn verify_payment_disclosure_inner(
    wallet_id: WalletId,
    disclosure: String,
) -> Result<PaymentDisclosureVerification> {
    let decoded = decode_payment_disclosure(&disclosure)?;
    let wallet_network = address_prefix_network_type(&wallet_id)?;
    if decoded.network_type != wallet_network {
        return Err(anyhow!(
            "Payment disclosure is for {:?}, but wallet endpoint is configured for {:?}",
            decoded.network_type,
            wallet_network
        ));
    }

    let endpoint_config = get_lightd_endpoint_config(wallet_id)?;
    let mut reversed = decoded.txid_bytes;
    reversed.reverse();
    let tx_hash_candidates = if reversed == decoded.txid_bytes {
        vec![decoded.txid_bytes]
    } else {
        vec![decoded.txid_bytes, reversed]
    };
    let raw = fetch_raw_transaction(endpoint_config, tx_hash_candidates).await?;
    let tx = Transaction::read(raw.bytes.as_slice(), BranchId::Nu5)
        .or_else(|_| Transaction::read(raw.bytes.as_slice(), BranchId::Canopy))
        .map_err(|e| anyhow!("Failed to parse transaction: {}", e))?;
    let txid_bytes = *tx.txid().as_ref();
    let block_height = BlockHeight::from_u32(raw.height.unwrap_or(0));
    let ock = zcash_note_encryption::OutgoingCipherKey(decoded.ock);

    match decoded.kind {
        DisclosureKind::Sapling => {
            let bundle = tx
                .sapling_bundle()
                .ok_or_else(|| anyhow!("Transaction has no Sapling outputs"))?;
            let output = bundle
                .shielded_outputs()
                .get(decoded.output_index as usize)
                .ok_or_else(|| {
                    anyhow!("Sapling output index {} out of range", decoded.output_index)
                })?;
            let network = PirateNetwork::new(wallet_network);
            let (note, address, memo) =
                try_sapling_output_recovery_with_ock(&network, block_height, &ock, output)
                    .ok_or_else(|| anyhow!("Failed to decrypt Sapling output with disclosure"))?;
            let memo_vec = memo.as_array().to_vec();
            Ok(PaymentDisclosureVerification {
                disclosure_type: DisclosureKind::Sapling.as_str().to_string(),
                txid: txid_string(&txid_bytes),
                output_index: decoded.output_index,
                address: PaymentAddress { inner: address }.encode_for_network(wallet_network),
                amount: note.value().inner(),
                memo: memo_to_text(&memo_vec),
                memo_hex: hex::encode(memo_vec),
            })
        }
        DisclosureKind::Orchard => {
            let bundle = tx
                .orchard_bundle()
                .ok_or_else(|| anyhow!("Transaction has no Orchard actions"))?;
            let action = bundle
                .actions()
                .get(decoded.output_index as usize)
                .ok_or_else(|| {
                    anyhow!("Orchard action index {} out of range", decoded.output_index)
                })?;
            let domain = OrchardDomain::for_action(action);
            let (note, address, memo) = try_output_recovery_with_ock(
                &domain,
                &ock,
                action,
                &action.encrypted_note().out_ciphertext,
            )
            .ok_or_else(|| anyhow!("Failed to decrypt Orchard action with disclosure"))?;
            let memo_vec = memo.to_vec();
            let address = (OrchardPaymentAddress { inner: address })
                .encode_for_network(wallet_network)
                .map_err(|e| anyhow!("Failed to encode Orchard address: {}", e))?;
            Ok(PaymentDisclosureVerification {
                disclosure_type: DisclosureKind::Orchard.as_str().to_string(),
                txid: txid_string(&txid_bytes),
                output_index: decoded.output_index,
                address,
                amount: note.value().inner(),
                memo: memo_to_text(&memo_vec),
                memo_hex: hex::encode(memo_vec),
            })
        }
    }
}
