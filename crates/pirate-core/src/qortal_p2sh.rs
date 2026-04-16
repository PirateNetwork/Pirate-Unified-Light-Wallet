#![allow(missing_docs, unexpected_cfgs)]

use crate::fees::{apply_dust_policy_add_to_fee, CHANGE_DUST_THRESHOLD};
use crate::keys::{ExtendedSpendingKey, OrchardExtendedSpendingKey, PaymentAddress};
use crate::memo::Memo;
use crate::params::sapling_prover;
use crate::selection::{NoteType, SelectableNote};
use crate::shielded_builder::SelectedSpendNoteRef;
use crate::transaction::PirateNetwork;
use crate::{Error, Result};
use orchard::builder::Builder as OrchardBuilder;
use orchard::bundle::Flags as OrchardFlags;
use orchard::keys::FullViewingKey as OrchardFullViewingKey;
use orchard::keys::SpendAuthorizingKey;
use orchard::tree::Anchor as OrchardAnchor;
use orchard::value::NoteValue as OrchardNoteValue;
use secp256k1::SecretKey;
use std::collections::HashMap;
use zcash_primitives::consensus::BlockHeight;
use zcash_primitives::legacy::{Script, TransparentAddress};
use zcash_primitives::memo::MemoBytes;
use zcash_primitives::sapling::prover::TxProver;
use zcash_primitives::transaction::components::amount::Amount;
use zcash_primitives::transaction::components::sapling::builder::SaplingBuilder;
use zcash_primitives::transaction::components::transparent::{
    self, Bundle as TransparentBundle, TxIn, TxOut,
};
use zcash_primitives::transaction::components::OutPoint;
use zcash_primitives::transaction::sighash::{
    signature_hash, SignableInput, TransparentAuthorizingContext, SIGHASH_ALL,
};
use zcash_primitives::transaction::txid::TxIdDigester;
use zcash_primitives::transaction::{Authorization as TxAuthorization, TransactionData, TxVersion};

#[derive(Debug, Clone)]
pub struct TransparentAuthContext {
    pub input_amounts: Vec<Amount>,
    pub input_scriptpubkeys: Vec<Script>,
}

impl transparent::Authorization for TransparentAuthContext {
    type ScriptSig = ();
}

impl TransparentAuthorizingContext for TransparentAuthContext {
    fn input_amounts(&self) -> Vec<Amount> {
        self.input_amounts.clone()
    }

    fn input_scriptpubkeys(&self) -> Vec<Script> {
        self.input_scriptpubkeys.clone()
    }
}

#[derive(Debug)]
pub struct CustomUnauthorized;

impl TxAuthorization for CustomUnauthorized {
    type TransparentAuth = TransparentAuthContext;
    type SaplingAuth = zcash_primitives::transaction::components::sapling::builder::Unauthorized;
    type OrchardAuth =
        orchard::builder::InProgress<orchard::builder::Unproven, orchard::builder::Unauthorized>;

    #[cfg(feature = "zfuture")]
    type TzeAuth = zcash_primitives::transaction::components::tze::builder::Unauthorized;
}

#[derive(Debug, Clone)]
pub enum QortalRecipient {
    Sapling {
        address: PaymentAddress,
        amount: u64,
        memo: Option<Memo>,
    },
    Orchard {
        address: orchard::Address,
        amount: u64,
        memo: Option<Memo>,
    },
    Transparent {
        address: TransparentAddress,
        amount: u64,
    },
}

pub struct QortalP2shFundingPlan<'a> {
    pub network_type: pirate_params::NetworkType,
    pub default_sapling_spending_key: &'a ExtendedSpendingKey,
    pub default_orchard_spending_key: Option<&'a OrchardExtendedSpendingKey>,
    pub sapling_spending_keys_by_id: HashMap<i64, ExtendedSpendingKey>,
    pub orchard_spending_keys_by_id: HashMap<i64, OrchardExtendedSpendingKey>,
    pub available_notes: Vec<SelectableNote>,
    pub target_height: u32,
    pub orchard_anchor: Option<OrchardAnchor>,
    pub change_diversifier_index: u32,
    pub recipients: Vec<QortalRecipient>,
    pub fee: u64,
    pub script_pubkey: Vec<u8>,
    pub script_each_recipient: bool,
}

pub struct QortalP2shRedeemPlan {
    pub network_type: pirate_params::NetworkType,
    pub target_height: u32,
    pub orchard_anchor: Option<OrchardAnchor>,
    pub funding_txid: [u8; 32],
    pub funding_coin: TxOut,
    pub recipients: Vec<QortalRecipient>,
    pub fee: u64,
    pub redeem_script: Vec<u8>,
    pub lock_time: u32,
    pub secret: Vec<u8>,
    pub privkey: SecretKey,
}

pub fn build_script_pubkey(script_bytes: &[u8]) -> Result<Script> {
    if script_bytes.is_empty() {
        return Err(Error::TransactionBuild(
            "Script pubkey bytes are empty".to_string(),
        ));
    }
    Ok(Script(script_bytes.to_vec()))
}

pub fn build_p2sh_script_sig(sig_bytes: &[u8], secret: &[u8], redeem_script: &[u8]) -> Script {
    if secret.is_empty() {
        let is_refund = [0x51u8];
        Script::default() << sig_bytes << &is_refund[..] << redeem_script
    } else {
        let is_redeem = [0x00u8];
        Script::default() << sig_bytes << secret << &is_redeem[..] << redeem_script
    }
}

fn memo_bytes_array(memo: &Option<Memo>) -> Result<[u8; 512]> {
    match memo {
        Some(memo) => Ok(*memo.to_memo_bytes()?.as_array()),
        None => Ok(*MemoBytes::empty().as_array()),
    }
}

struct OutputBuildOptions<'a> {
    script_pubkey: Option<&'a Script>,
    script_each_recipient: bool,
    sapling_ovk: zcash_primitives::sapling::keys::OutgoingViewingKey,
    orchard_ovk: Option<orchard::keys::OutgoingViewingKey>,
}

fn add_outputs_and_script(
    sapling_builder: &mut SaplingBuilder<PirateNetwork>,
    orchard_builder: &mut Option<OrchardBuilder>,
    transparent_vout: &mut Vec<TxOut>,
    recipients: &[QortalRecipient],
    options: OutputBuildOptions<'_>,
) -> Result<(bool, bool)> {
    let mut has_sapling = false;
    let mut has_orchard = false;

    for recipient in recipients {
        match recipient {
            QortalRecipient::Sapling {
                address,
                amount,
                memo,
            } => {
                has_sapling = true;
                let memo_bytes = match memo {
                    Some(memo) => memo.to_memo_bytes()?,
                    None => MemoBytes::empty(),
                };
                sapling_builder
                    .add_output(
                        &mut rand::rngs::OsRng,
                        Some(options.sapling_ovk),
                        address.inner,
                        zcash_primitives::sapling::value::NoteValue::from_raw(*amount),
                        memo_bytes,
                    )
                    .map_err(|e| {
                        Error::TransactionBuild(format!("Failed to add Sapling output: {:?}", e))
                    })?;
            }
            QortalRecipient::Orchard {
                address,
                amount,
                memo,
            } => {
                has_orchard = true;
                let builder = orchard_builder.as_mut().ok_or_else(|| {
                    Error::TransactionBuild("Orchard builder not initialized".to_string())
                })?;
                builder
                    .add_recipient(
                        options.orchard_ovk.clone(),
                        *address,
                        OrchardNoteValue::from_raw(*amount),
                        Some(memo_bytes_array(memo)?),
                    )
                    .map_err(|e| {
                        Error::TransactionBuild(format!("Failed to add Orchard output: {:?}", e))
                    })?;
            }
            QortalRecipient::Transparent { address, amount } => {
                transparent_vout.push(TxOut {
                    value: Amount::from_u64(*amount).map_err(|_| {
                        Error::InvalidAmount("Transparent amount out of range".to_string())
                    })?,
                    script_pubkey: address.script(),
                });
            }
        }

        if options.script_each_recipient {
            let script_pubkey = options.script_pubkey.ok_or_else(|| {
                Error::TransactionBuild("Missing script output bytes".to_string())
            })?;
            transparent_vout.push(TxOut {
                value: Amount::zero(),
                script_pubkey: script_pubkey.clone(),
            });
        }
    }

    if !options.script_each_recipient {
        if let Some(script_pubkey) = options.script_pubkey {
            transparent_vout.push(TxOut {
                value: Amount::zero(),
                script_pubkey: script_pubkey.clone(),
            });
        }
    }

    Ok((has_sapling, has_orchard))
}

pub fn build_qortal_p2sh_funding_transaction(
    plan: QortalP2shFundingPlan<'_>,
) -> Result<crate::shielded_builder::SignedShieldedTransaction> {
    let script_pubkey = build_script_pubkey(&plan.script_pubkey)?;
    let total_value = plan
        .recipients
        .iter()
        .map(|recipient| match recipient {
            QortalRecipient::Sapling { amount, .. }
            | QortalRecipient::Orchard { amount, .. }
            | QortalRecipient::Transparent { amount, .. } => *amount,
        })
        .sum::<u64>();

    let target_total = total_value
        .checked_add(plan.fee)
        .ok_or_else(|| Error::AmountOverflow("Output + fee overflow".to_string()))?;

    let selector =
        crate::selection::NoteSelector::new(crate::selection::SelectionStrategy::SmallestFirst);
    let selection = selector.select_notes(plan.available_notes, total_value, plan.fee)?;
    let change = selection
        .total_value
        .checked_sub(target_total)
        .ok_or_else(|| {
            Error::InsufficientFunds("Insufficient funds for Qortal P2SH funding".to_string())
        })?;
    let effective = apply_dust_policy_add_to_fee(plan.fee, change)?;
    let change = effective.change;
    let network = PirateNetwork::new(plan.network_type);
    let target_height = BlockHeight::from_u32(plan.target_height);
    let sapling_ovk = plan
        .default_sapling_spending_key
        .to_extended_fvk()
        .outgoing_viewing_key();
    let orchard_ovk = plan
        .default_orchard_spending_key
        .map(|sk| sk.to_extended_fvk().to_ovk());

    let has_orchard_spends = selection
        .notes
        .iter()
        .any(|note| note.note_type == NoteType::Orchard);
    let has_orchard_outputs = plan
        .recipients
        .iter()
        .any(|recipient| matches!(recipient, QortalRecipient::Orchard { .. }));
    let use_orchard_change = has_orchard_spends || has_orchard_outputs;
    let use_sapling_internal_change =
        crate::sapling_internal_change_active(plan.network_type, u64::from(plan.target_height));

    let mut sapling_builder = SaplingBuilder::new(network.clone(), target_height);
    let mut orchard_builder = if has_orchard_spends || has_orchard_outputs || use_orchard_change {
        Some(OrchardBuilder::new(
            OrchardFlags::from_parts(
                has_orchard_spends,
                has_orchard_outputs || use_orchard_change,
            ),
            plan.orchard_anchor
                .ok_or_else(|| Error::TransactionBuild("Missing Orchard anchor".to_string()))?,
        ))
    } else {
        None
    };
    let mut transparent_vout = Vec::new();
    let mut orchard_spend_auth_keys = Vec::new();
    let mut spent_notes = Vec::new();
    let mut first_legacy_sapling_change: Option<(
        zcash_primitives::sapling::keys::OutgoingViewingKey,
        zcash_primitives::sapling::PaymentAddress,
    )> = None;
    let mut rng = rand::rngs::OsRng;

    for note in selection.notes {
        spent_notes.push(SelectedSpendNoteRef {
            note_type: note.note_type,
            txid: note.txid.clone(),
            output_index: note.output_index,
            key_id: note.key_id,
            nullifier: note.nullifier.clone(),
        });

        match note.note_type {
            NoteType::Sapling => {
                let diversifier = note.diversifier.ok_or_else(|| {
                    Error::TransactionBuild("Missing Sapling diversifier".to_string())
                })?;
                let sapling_note = note
                    .note
                    .ok_or_else(|| Error::TransactionBuild("Missing Sapling note".to_string()))?;
                let merkle_path = note.merkle_path.ok_or_else(|| {
                    Error::TransactionBuild("Missing Sapling merkle path".to_string())
                })?;
                let base_key = note
                    .key_id
                    .and_then(|key_id| plan.sapling_spending_keys_by_id.get(&key_id))
                    .unwrap_or(plan.default_sapling_spending_key);
                let recipient = sapling_note.recipient();
                if first_legacy_sapling_change.is_none() {
                    first_legacy_sapling_change =
                        Some((base_key.to_extended_fvk().outgoing_viewing_key(), recipient));
                }
                let mut spend_key = base_key.clone();
                let external_matches = base_key
                    .to_extended_fvk()
                    .address_from_diversifier(diversifier.0)
                    .map(|addr| addr.inner == recipient)
                    .unwrap_or(false);
                if !external_matches {
                    let internal_candidate = base_key.derive_internal();
                    let internal_matches = internal_candidate
                        .to_extended_fvk()
                        .address_from_diversifier(diversifier.0)
                        .map(|addr| addr.inner == recipient)
                        .unwrap_or(false);
                    if internal_matches {
                        spend_key = internal_candidate;
                    }
                }
                sapling_builder
                    .add_spend(
                        &mut rng,
                        spend_key.inner().clone(),
                        diversifier,
                        sapling_note,
                        merkle_path,
                    )
                    .map_err(|e| {
                        Error::TransactionBuild(format!("Failed to add Sapling spend: {:?}", e))
                    })?;
            }
            NoteType::Orchard => {
                let orchard_note = note
                    .orchard_note
                    .ok_or_else(|| Error::TransactionBuild("Missing Orchard note".to_string()))?;
                let merkle_path = note.orchard_merkle_path.ok_or_else(|| {
                    Error::TransactionBuild("Missing Orchard merkle path".to_string())
                })?;
                let orchard_key = note
                    .key_id
                    .and_then(|key_id| plan.orchard_spending_keys_by_id.get(&key_id))
                    .or(plan.default_orchard_spending_key)
                    .ok_or_else(|| {
                        Error::TransactionBuild("Missing Orchard spending key".to_string())
                    })?;
                let fvk: OrchardFullViewingKey = (&orchard_key.inner).into();
                orchard_spend_auth_keys.push(SpendAuthorizingKey::from(&orchard_key.inner));
                orchard_builder
                    .as_mut()
                    .expect("orchard builder")
                    .add_spend(fvk, orchard_note, merkle_path)
                    .map_err(|e| {
                        Error::TransactionBuild(format!("Failed to add Orchard spend: {:?}", e))
                    })?;
            }
        }
    }

    add_outputs_and_script(
        &mut sapling_builder,
        &mut orchard_builder,
        &mut transparent_vout,
        &plan.recipients,
        OutputBuildOptions {
            script_pubkey: Some(&script_pubkey),
            script_each_recipient: plan.script_each_recipient,
            sapling_ovk,
            orchard_ovk: orchard_ovk.clone(),
        },
    )?;

    if change >= CHANGE_DUST_THRESHOLD {
        if use_orchard_change {
            let orchard_key = plan.default_orchard_spending_key.ok_or_else(|| {
                Error::TransactionBuild("Missing Orchard spending key for change".to_string())
            })?;
            let address = orchard_key
                .to_extended_fvk()
                .address_at_internal(plan.change_diversifier_index);
            orchard_builder
                .as_mut()
                .expect("orchard builder")
                .add_recipient(
                    orchard_ovk,
                    address.inner,
                    OrchardNoteValue::from_raw(change),
                    Some(*MemoBytes::empty().as_array()),
                )
                .map_err(|e| {
                    Error::TransactionBuild(format!("Failed to add Orchard change: {:?}", e))
                })?;
        } else if use_sapling_internal_change {
            let address = plan
                .default_sapling_spending_key
                .to_internal_fvk()
                .derive_address(plan.change_diversifier_index);
            sapling_builder
                .add_output(
                    &mut rng,
                    Some(sapling_ovk),
                    address.inner,
                    zcash_primitives::sapling::value::NoteValue::from_raw(change),
                    MemoBytes::empty(),
                )
                .map_err(|e| {
                    Error::TransactionBuild(format!("Failed to add Sapling change: {:?}", e))
                })?;
        } else {
            let (legacy_ovk, legacy_address) = first_legacy_sapling_change.ok_or_else(|| {
                Error::TransactionBuild(
                    "Sapling legacy change requires a selected Sapling spend".to_string(),
                )
            })?;
            sapling_builder
                .add_output(
                    &mut rng,
                    Some(legacy_ovk),
                    legacy_address,
                    zcash_primitives::sapling::value::NoteValue::from_raw(change),
                    MemoBytes::empty(),
                )
                .map_err(|e| {
                    Error::TransactionBuild(format!("Failed to add legacy Sapling change: {:?}", e))
                })?;
        }
    }

    let prover = sapling_prover();
    let mut sapling_ctx = prover.new_sapling_proving_context();
    let sapling_bundle = sapling_builder
        .build(&prover, &mut sapling_ctx, &mut rng, target_height, None)
        .map_err(|e| Error::TransactionBuild(format!("Failed to build Sapling bundle: {:?}", e)))?;

    let orchard_bundle = if let Some(builder) = orchard_builder {
        Some(builder.build(&mut rng).map_err(|e| {
            Error::TransactionBuild(format!("Failed to build Orchard bundle: {:?}", e))
        })?)
    } else {
        None
    };

    let transparent_bundle = if transparent_vout.is_empty() {
        None
    } else {
        Some(TransparentBundle {
            vin: vec![],
            vout: transparent_vout.clone(),
            authorization: TransparentAuthContext {
                input_amounts: vec![],
                input_scriptpubkeys: vec![],
            },
        })
    };

    let branch_id = zcash_primitives::consensus::BranchId::for_height(&network, target_height);
    let tx_version = TxVersion::suggested_for_branch(branch_id);
    let expiry_height = target_height + 20;

    let unauth_tx = TransactionData::<CustomUnauthorized>::from_parts(
        tx_version,
        branch_id,
        0,
        expiry_height,
        transparent_bundle,
        None,
        sapling_bundle,
        orchard_bundle,
        #[cfg(feature = "zfuture")]
        None,
    );

    let txid_parts = unauth_tx.digest(TxIdDigester);
    let shielded_sighash = signature_hash(&unauth_tx, &SignableInput::Shielded, &txid_parts);

    let signed_transparent_bundle =
        unauth_tx
            .transparent_bundle()
            .as_ref()
            .map(|bundle| TransparentBundle {
                vin: vec![],
                vout: bundle.vout.clone(),
                authorization: transparent::Authorized,
            });

    let signed_sapling_bundle = match unauth_tx.sapling_bundle().cloned() {
        Some(bundle) => Some(
            bundle
                .apply_signatures(
                    &prover,
                    &mut sapling_ctx,
                    &mut rng,
                    shielded_sighash.as_ref(),
                )
                .map_err(|e| {
                    Error::TransactionBuild(format!("Failed to sign Sapling bundle: {:?}", e))
                })?
                .0,
        ),
        None => None,
    };

    let orchard_proving_key = orchard::circuit::ProvingKey::build();
    let signed_orchard_bundle = match unauth_tx.orchard_bundle().cloned() {
        Some(bundle) => Some(
            bundle
                .create_proof(&orchard_proving_key, &mut rng)
                .map_err(|e| {
                    Error::TransactionBuild(format!("Failed to prove Orchard bundle: {:?}", e))
                })?
                .apply_signatures(rng, *shielded_sighash.as_ref(), &orchard_spend_auth_keys)
                .map_err(|e| {
                    Error::TransactionBuild(format!("Failed to sign Orchard bundle: {:?}", e))
                })?,
        ),
        None => None,
    };

    let authorized_tx = TransactionData::from_parts(
        tx_version,
        branch_id,
        0,
        expiry_height,
        signed_transparent_bundle,
        None,
        signed_sapling_bundle,
        signed_orchard_bundle,
        #[cfg(feature = "zfuture")]
        None,
    );

    let tx = authorized_tx
        .freeze()
        .map_err(|e| Error::TransactionBuild(format!("Failed to finalize transaction: {:?}", e)))?;
    let mut raw_tx = Vec::new();
    tx.write(&mut raw_tx)
        .map_err(|e| Error::TransactionBuild(format!("Failed to serialize transaction: {}", e)))?;

    Ok(crate::shielded_builder::SignedShieldedTransaction {
        txid: tx.txid(),
        size: raw_tx.len(),
        raw_tx,
        spent_notes,
    })
}

pub fn build_qortal_p2sh_redeem_transaction(
    plan: QortalP2shRedeemPlan,
) -> Result<crate::transaction::SignedTransaction> {
    let script_code = Script(plan.redeem_script.clone());
    let sequence = if plan.lock_time > 0 {
        0xFFFFFFFE
    } else {
        0xFFFFFFFF
    };
    let network = PirateNetwork::new(plan.network_type);
    let target_height = BlockHeight::from_u32(plan.target_height);
    let branch_id = zcash_primitives::consensus::BranchId::for_height(&network, target_height);
    let tx_version = TxVersion::suggested_for_branch(branch_id);
    let expiry_height = target_height + 20;

    let mut sapling_builder = SaplingBuilder::new(network.clone(), target_height);
    let has_orchard_outputs = plan
        .recipients
        .iter()
        .any(|recipient| matches!(recipient, QortalRecipient::Orchard { .. }));
    let mut orchard_builder = if has_orchard_outputs {
        Some(OrchardBuilder::new(
            OrchardFlags::from_parts(false, true),
            plan.orchard_anchor
                .ok_or_else(|| Error::TransactionBuild("Missing Orchard anchor".to_string()))?,
        ))
    } else {
        None
    };
    let mut transparent_vout = Vec::new();
    let mut rng = rand::rngs::OsRng;

    add_outputs_and_script(
        &mut sapling_builder,
        &mut orchard_builder,
        &mut transparent_vout,
        &plan.recipients,
        OutputBuildOptions {
            script_pubkey: None,
            script_each_recipient: false,
            sapling_ovk: zcash_primitives::sapling::keys::OutgoingViewingKey([0u8; 32]),
            orchard_ovk: None,
        },
    )?;

    let fee_amount =
        Amount::from_u64(plan.fee).map_err(|_| Error::InvalidAmount("Invalid fee".to_string()))?;
    let outputs_total = transparent_vout
        .iter()
        .try_fold(Amount::zero(), |acc, out| {
            (acc + out.value).ok_or(Error::AmountOverflow(
                "Transparent output overflow".to_string(),
            ))
        })?;
    let required_total = (outputs_total + fee_amount)
        .ok_or_else(|| Error::AmountOverflow("Transparent output + fee overflow".to_string()))?;

    if plan.funding_coin.value < required_total {
        return Err(Error::InsufficientFunds(
            "Funding output value is less than outputs plus fee".to_string(),
        ));
    }

    let prover = sapling_prover();
    let mut sapling_ctx = prover.new_sapling_proving_context();
    let sapling_bundle = sapling_builder
        .build(&prover, &mut sapling_ctx, &mut rng, target_height, None)
        .map_err(|e| Error::TransactionBuild(format!("Failed to build Sapling bundle: {:?}", e)))?;
    let orchard_bundle = if let Some(builder) = orchard_builder {
        Some(builder.build(&mut rng).map_err(|e| {
            Error::TransactionBuild(format!("Failed to build Orchard bundle: {:?}", e))
        })?)
    } else {
        None
    };

    let transparent_bundle = Some(TransparentBundle {
        vin: vec![TxIn {
            prevout: OutPoint::new(plan.funding_txid, 0),
            script_sig: (),
            sequence,
        }],
        vout: transparent_vout.clone(),
        authorization: TransparentAuthContext {
            input_amounts: vec![plan.funding_coin.value],
            input_scriptpubkeys: vec![plan.funding_coin.script_pubkey.clone()],
        },
    });

    let unauth_tx = TransactionData::<CustomUnauthorized>::from_parts(
        tx_version,
        branch_id,
        plan.lock_time,
        expiry_height,
        transparent_bundle,
        None,
        sapling_bundle,
        orchard_bundle,
        #[cfg(feature = "zfuture")]
        None,
    );

    let txid_parts = unauth_tx.digest(TxIdDigester);
    let sighash = signature_hash(
        &unauth_tx,
        &SignableInput::Transparent {
            hash_type: SIGHASH_ALL,
            index: 0,
            script_code: &script_code,
            script_pubkey: &plan.funding_coin.script_pubkey,
            value: plan.funding_coin.value,
        },
        &txid_parts,
    );
    let msg = secp256k1::Message::from_slice(sighash.as_ref())
        .map_err(|e| Error::TransactionBuild(format!("Invalid signature hash: {}", e)))?;
    let secp = secp256k1::Secp256k1::signing_only();
    let sig = secp.sign_ecdsa(&msg, &plan.privkey);
    let mut sig_bytes = sig.serialize_der().to_vec();
    sig_bytes.push(SIGHASH_ALL);
    let script_sig = build_p2sh_script_sig(&sig_bytes, &plan.secret, &plan.redeem_script);

    let shielded_sighash = signature_hash(&unauth_tx, &SignableInput::Shielded, &txid_parts);

    let signed_sapling_bundle = match unauth_tx.sapling_bundle().cloned() {
        Some(bundle) => Some(
            bundle
                .apply_signatures(
                    &prover,
                    &mut sapling_ctx,
                    &mut rng,
                    shielded_sighash.as_ref(),
                )
                .map_err(|e| {
                    Error::TransactionBuild(format!("Failed to sign Sapling bundle: {:?}", e))
                })?
                .0,
        ),
        None => None,
    };

    let orchard_proving_key = orchard::circuit::ProvingKey::build();
    let signed_orchard_bundle = match unauth_tx.orchard_bundle().cloned() {
        Some(bundle) => Some(
            bundle
                .create_proof(&orchard_proving_key, &mut rng)
                .map_err(|e| {
                    Error::TransactionBuild(format!("Failed to prove Orchard bundle: {:?}", e))
                })?
                .apply_signatures(rng, *shielded_sighash.as_ref(), &[])
                .map_err(|e| {
                    Error::TransactionBuild(format!("Failed to sign Orchard bundle: {:?}", e))
                })?,
        ),
        None => None,
    };

    let signed_transparent_bundle = Some(TransparentBundle {
        vin: vec![TxIn {
            prevout: OutPoint::new(plan.funding_txid, 0),
            script_sig,
            sequence,
        }],
        vout: transparent_vout,
        authorization: transparent::Authorized,
    });

    let authorized_tx = TransactionData::from_parts(
        tx_version,
        branch_id,
        plan.lock_time,
        expiry_height,
        signed_transparent_bundle,
        None,
        signed_sapling_bundle,
        signed_orchard_bundle,
        #[cfg(feature = "zfuture")]
        None,
    );
    let tx = authorized_tx
        .freeze()
        .map_err(|e| Error::TransactionBuild(format!("Failed to finalize transaction: {:?}", e)))?;
    let mut raw_tx = Vec::new();
    tx.write(&mut raw_tx)
        .map_err(|e| Error::TransactionBuild(format!("Failed to serialize transaction: {}", e)))?;

    Ok(crate::transaction::SignedTransaction {
        txid: tx.txid(),
        size: raw_tx.len(),
        raw_tx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::SelectableNote;
    use zcash_primitives::consensus::BranchId;
    use zcash_primitives::legacy::TransparentAddress;
    use zcash_primitives::transaction::components::TxOut;
    use zcash_primitives::transaction::Transaction;

    fn sample_spending_key() -> ExtendedSpendingKey {
        let mnemonic = ExtendedSpendingKey::generate_mnemonic(None);
        ExtendedSpendingKey::from_mnemonic(&mnemonic).unwrap()
    }

    fn transparent_recipient(tag: u8, amount: u64) -> QortalRecipient {
        QortalRecipient::Transparent {
            address: TransparentAddress::PublicKey([tag; 20]),
            amount,
        }
    }

    fn funding_coin(value: u64) -> TxOut {
        TxOut {
            value: Amount::from_u64(value).unwrap(),
            script_pubkey: TransparentAddress::Script([9u8; 20]).script(),
        }
    }

    fn redeem_plan(lock_time: u32, secret: Vec<u8>) -> QortalP2shRedeemPlan {
        QortalP2shRedeemPlan {
            network_type: pirate_params::NetworkType::Mainnet,
            target_height: 200_000,
            orchard_anchor: None,
            funding_txid: [4u8; 32],
            funding_coin: funding_coin(50_000),
            recipients: vec![transparent_recipient(7, 20_000)],
            fee: 10_000,
            redeem_script: vec![0x51, 0x21, 0x02],
            lock_time,
            secret,
            privkey: SecretKey::from_slice(&[3u8; 32]).unwrap(),
        }
    }

    #[test]
    fn funding_plan_rejects_insufficient_funds() {
        let sapling_key = sample_spending_key();
        let result = build_qortal_p2sh_funding_transaction(QortalP2shFundingPlan {
            network_type: pirate_params::NetworkType::Mainnet,
            default_sapling_spending_key: &sapling_key,
            default_orchard_spending_key: None,
            sapling_spending_keys_by_id: HashMap::new(),
            orchard_spending_keys_by_id: HashMap::new(),
            available_notes: vec![SelectableNote::new(
                10_000,
                vec![0u8; 32],
                1,
                vec![1u8; 32],
                0,
            )],
            target_height: 200_000,
            orchard_anchor: None,
            change_diversifier_index: 0,
            recipients: vec![transparent_recipient(5, 20_000)],
            fee: 10_000,
            script_pubkey: vec![0x6a, 0x00],
            script_each_recipient: true,
        });

        let err = result.unwrap_err().to_string();
        assert!(err.contains("Insufficient funds"));
    }

    #[test]
    fn funding_helper_adds_script_output_per_recipient() {
        let sapling_key = sample_spending_key();
        let network = PirateNetwork::new(pirate_params::NetworkType::Mainnet);
        let target_height = BlockHeight::from_u32(200_000);
        let mut sapling_builder = SaplingBuilder::new(network, target_height);
        let mut orchard_builder = None;
        let mut transparent_vout = Vec::new();
        let script_pubkey = build_script_pubkey(&[0x6a, 0x00]).unwrap();
        let recipients = vec![
            transparent_recipient(5, 20_000),
            transparent_recipient(6, 30_000),
        ];
        let sapling_ovk = sapling_key.to_extended_fvk().outgoing_viewing_key();

        let (has_sapling, has_orchard) = add_outputs_and_script(
            &mut sapling_builder,
            &mut orchard_builder,
            &mut transparent_vout,
            &recipients,
            OutputBuildOptions {
                script_pubkey: Some(&script_pubkey),
                script_each_recipient: true,
                sapling_ovk,
                orchard_ovk: None,
            },
        )
        .unwrap();

        assert!(!has_sapling);
        assert!(!has_orchard);
        assert_eq!(transparent_vout.len(), 4);
        assert_eq!(transparent_vout[0].value, Amount::from_u64(20_000).unwrap());
        assert_eq!(transparent_vout[1].value, Amount::zero());
        assert_eq!(transparent_vout[2].value, Amount::from_u64(30_000).unwrap());
        assert_eq!(transparent_vout[3].value, Amount::zero());
        assert_eq!(transparent_vout[1].script_pubkey, script_pubkey);
        assert_eq!(transparent_vout[3].script_pubkey, script_pubkey);
    }

    #[test]
    fn redeem_build_includes_secret_in_script_sig() {
        let secret = vec![8u8, 9u8, 10u8];
        let signed = build_qortal_p2sh_redeem_transaction(redeem_plan(0, secret.clone())).unwrap();
        let tx = Transaction::read(&signed.raw_tx[..], BranchId::Nu5).unwrap();
        let script_sig = &tx.transparent_bundle().unwrap().vin[0].script_sig.0;

        assert!(script_sig
            .windows(secret.len())
            .any(|window| window == secret.as_slice()));
        assert!(script_sig.ends_with(&[0x51, 0x21, 0x02]));
    }

    #[test]
    fn refund_build_uses_refund_branch_without_secret() {
        let signed = build_qortal_p2sh_redeem_transaction(redeem_plan(10, Vec::new())).unwrap();
        let tx = Transaction::read(&signed.raw_tx[..], BranchId::Nu5).unwrap();
        let script_sig = &tx.transparent_bundle().unwrap().vin[0].script_sig.0;

        assert!(script_sig.contains(&0x51));
        assert!(script_sig.ends_with(&[0x51, 0x21, 0x02]));
    }
}
