use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jstring};
use jni::JNIEnv;
use pirate_core::mnemonic::{inspect_mnemonic, mnemonic_from_entropy};
use pirate_wallet_service::{
    AddressInfo, KeyExportInfo, KeyGroupInfo, MnemonicLanguage, NodeTestResult,
    QortalP2shRedeemRequest, QortalP2shSendRequest, QortalSendRequest, SyncMode, SyncStatus,
    TunnelMode, WalletMeta, WalletService, WalletServiceRequest,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::{Mutex, MutexGuard};

#[derive(Default)]
struct AdapterState {
    wallet_id: Option<String>,
    server_uri: Option<String>,
}

static STATE: Mutex<AdapterState> = Mutex::new(AdapterState {
    wallet_id: None,
    server_uri: None,
});

fn state() -> MutexGuard<'static, AdapterState> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn service() -> WalletService {
    WalletService::new()
}

fn execute(request: WalletServiceRequest) -> Result<Value> {
    service().execute_blocking(request)
}

fn read_string(env: &mut JNIEnv<'_>, value: JString<'_>) -> Result<String> {
    Ok(env
        .get_string(&value)
        .map_err(|err| anyhow!("Failed to read JNI string: {}", err))?
        .to_string_lossy()
        .into_owned())
}

fn write_string(env: &mut JNIEnv<'_>, value: String) -> jstring {
    let fallback = "{\"error\":\"Failed to create JNI response\"}";
    env.new_string(&value)
        .or_else(|_| env.new_string(fallback))
        .expect("static fallback JNI string is valid")
        .into_raw()
}

fn error_json(error: impl std::fmt::Display) -> String {
    json!({ "error": error.to_string() }).to_string()
}

fn result_json(result: Result<String>) -> String {
    result.unwrap_or_else(error_json)
}

fn active_wallet_id() -> Result<String> {
    state()
        .wallet_id
        .clone()
        .ok_or_else(|| anyhow!("Qortal wallet storage has not been initialized"))
}

fn deterministic_wallet_name(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    format!("Qortal {}", &hex::encode(digest)[..16])
}

fn wallet_meta_by_id(wallet_id: &str) -> Result<WalletMeta> {
    let wallets: Vec<WalletMeta> =
        serde_json::from_value(execute(WalletServiceRequest::ListWallets)?)?;
    wallets
        .into_iter()
        .find(|wallet| wallet.id == wallet_id)
        .ok_or_else(|| {
            anyhow!(
                "Wallet {} was not registered after initialization",
                wallet_id
            )
        })
}

fn configure_storage(base_dir: String, passphrase: String) -> Result<String> {
    execute(WalletServiceRequest::ConfigureWalletStorage {
        base_dir,
        passphrase,
    })?;
    execute(WalletServiceRequest::SetTunnel {
        mode: TunnelMode::Direct,
    })?;
    *state() = AdapterState::default();
    Ok(json!({ "initialized": true }).to_string())
}

fn select_wallet(wallet: &WalletMeta, server_uri: &str) -> Result<()> {
    execute(WalletServiceRequest::SwitchWallet {
        wallet_id: wallet.id.clone(),
    })?;
    execute(WalletServiceRequest::SetLightdEndpoint {
        wallet_id: wallet.id.clone(),
        url: server_uri.to_string(),
        tls_pin_opt: None,
    })?;
    let mut adapter = state();
    adapter.wallet_id = Some(wallet.id.clone());
    adapter.server_uri = Some(server_uri.to_string());
    Ok(())
}

fn reported_wallet_height(wallet_id: &str, status: &SyncStatus) -> Result<u64> {
    if status.local_height > 0 {
        return Ok(status.local_height);
    }
    let wallets: Vec<WalletMeta> =
        serde_json::from_value(execute(WalletServiceRequest::ListWallets)?)?;
    let wallet = wallets
        .into_iter()
        .find(|wallet| wallet.id == wallet_id)
        .ok_or_else(|| anyhow!("Active Qortal wallet is not registered"))?;
    Ok(u64::from(wallet.birthday_height))
}

fn reported_chain_tip(wallet_id: String, status: &SyncStatus) -> Result<u64> {
    if status.target_height > 0 {
        return Ok(status.target_height);
    }
    let server_uri = state()
        .server_uri
        .clone()
        .ok_or_else(|| anyhow!("Qortal lightwalletd endpoint is not configured"))?;
    let result: NodeTestResult =
        serde_json::from_value(execute(WalletServiceRequest::TestNode {
            url: server_uri,
            tls_pin: None,
        })?)?;
    result.latest_block_height.ok_or_else(|| {
        anyhow!(
            "Unable to query Qortal lightwalletd tip for wallet {}: {}",
            wallet_id,
            result
                .error_message
                .unwrap_or_else(|| "height was missing".to_string())
        )
    })
}

fn initialize_from_seed(server_uri: String, seed: String, birthday: String) -> Result<String> {
    let birthday = birthday
        .trim()
        .parse::<u32>()
        .with_context(|| format!("Invalid birthday height: {birthday}"))?;
    let name = deterministic_wallet_name(&seed);
    let wallets: Vec<WalletMeta> =
        serde_json::from_value(execute(WalletServiceRequest::ListWallets)?)?;

    let wallet = if let Some(existing) = wallets.into_iter().find(|wallet| wallet.name == name) {
        existing
    } else {
        let wallet_id: String =
            serde_json::from_value(execute(WalletServiceRequest::RestoreWallet {
                name,
                mnemonic: seed.clone(),
                birthday_opt: Some(birthday),
                mnemonic_language: Some(MnemonicLanguage::English),
            })?)?;
        wallet_meta_by_id(&wallet_id)?
    };
    select_wallet(&wallet, &server_uri)?;
    Ok(json!({
        "seed": seed,
        "birthday": birthday,
        "wallet_id": wallet.id,
    })
    .to_string())
}

fn initialize_existing(server_uri: String) -> Result<String> {
    let wallets: Vec<WalletMeta> =
        serde_json::from_value(execute(WalletServiceRequest::ListWallets)?)?;
    let wallet = wallets.into_iter().next().ok_or_else(|| {
        anyhow!(
            "Legacy base64 wallet data cannot be opened by the unified core; restore this namespace once from its deterministic seed"
        )
    })?;
    select_wallet(&wallet, &server_uri)?;
    Ok(json!({
        "initialized": true,
        "initalized": true,
        "error": "none",
    })
    .to_string())
}

fn create_wallet(server_uri: String) -> Result<String> {
    let node: NodeTestResult = serde_json::from_value(execute(WalletServiceRequest::TestNode {
        url: server_uri.clone(),
        tls_pin: None,
    })?)?;
    let birthday = node
        .latest_block_height
        .and_then(|height| u32::try_from(height).ok())
        .ok_or_else(|| {
            anyhow!(
                "Unable to determine birthday height for new Qortal wallet: {}",
                node.error_message
                    .unwrap_or_else(|| "height was missing or out of range".to_string())
            )
        })?;
    let wallet_id: String = serde_json::from_value(execute(WalletServiceRequest::CreateWallet {
        name: "Qortal wallet".to_string(),
        birthday_opt: Some(birthday),
        mnemonic_language: Some(MnemonicLanguage::English),
    })?)?;
    let wallet = wallet_meta_by_id(&wallet_id)?;
    let seed: String = serde_json::from_value(execute(WalletServiceRequest::ExportSeedRaw {
        wallet_id: wallet.id.clone(),
        mnemonic_language: Some(MnemonicLanguage::English),
    })?)?;
    select_wallet(&wallet, &server_uri)?;
    Ok(json!({
        "seed": seed,
        "birthday": birthday,
        "wallet_id": wallet.id,
    })
    .to_string())
}

fn send(args: &str, wallet_id: String) -> Result<Value> {
    let request: QortalSendRequest =
        serde_json::from_str(args).map_err(|err| anyhow!("Invalid send JSON: {}", err))?;
    let txid: String = serde_json::from_value(execute(WalletServiceRequest::QortalSend {
        wallet_id,
        request,
    })?)?;
    Ok(json!({ "txid": txid }))
}

fn export_primary_key(wallet_id: String) -> Result<Value> {
    let groups: Vec<KeyGroupInfo> =
        serde_json::from_value(execute(WalletServiceRequest::ListKeyGroups {
            wallet_id: wallet_id.clone(),
        })?)?;
    let group = groups
        .into_iter()
        .find(|group| group.spendable)
        .ok_or_else(|| anyhow!("No spendable key group is available"))?;
    let keys: KeyExportInfo =
        serde_json::from_value(execute(WalletServiceRequest::ExportKeyGroupKeys {
            wallet_id: wallet_id.clone(),
            key_id: group.id,
        })?)?;
    let addresses: Vec<AddressInfo> =
        serde_json::from_value(execute(WalletServiceRequest::ListAddresses {
            wallet_id: wallet_id.clone(),
        })?)?;
    let address = if let Some(sapling) = addresses.into_iter().find(|entry| {
        entry.address.starts_with("zs")
            || entry.address.starts_with("ztestsapling")
            || entry.address.starts_with("zregtestsapling")
    }) {
        sapling.address
    } else {
        serde_json::from_value(execute(WalletServiceRequest::CurrentReceiveAddress {
            wallet_id,
        })?)?
    };

    Ok(json!([{
        "address": address,
        "private_key": keys.sapling_spending_key.or(keys.orchard_spending_key),
        "viewing_key": keys.sapling_viewing_key.or(keys.orchard_viewing_key),
    }]))
}

fn execute_legacy_command(command: &str, args: &str) -> Result<String> {
    let wallet_id = active_wallet_id()?;
    let normalized = command.trim().to_ascii_lowercase();
    let value = match normalized.as_str() {
        "sync" => {
            execute(WalletServiceRequest::StartSync {
                wallet_id,
                mode: SyncMode::Compact,
            })?;
            json!({ "result": "success" })
        }
        "syncstatus" => execute(WalletServiceRequest::QortalSyncStatus { wallet_id })?,
        "height" => {
            let status: SyncStatus =
                serde_json::from_value(execute(WalletServiceRequest::SyncStatus {
                    wallet_id: wallet_id.clone(),
                })?)?;
            json!({ "height": reported_wallet_height(&wallet_id, &status)? })
        }
        "info" => {
            let status: SyncStatus =
                serde_json::from_value(execute(WalletServiceRequest::SyncStatus {
                    wallet_id: wallet_id.clone(),
                })?)?;
            json!({ "latest_block_height": reported_chain_tip(wallet_id, &status)? })
        }
        "balance" => execute(WalletServiceRequest::QortalBalance { wallet_id })?,
        "list" => execute(WalletServiceRequest::QortalListTransactions {
            wallet_id,
            limit: None,
        })?,
        "send" => send(args, wallet_id)?,
        "sendp2sh" => {
            let request: QortalP2shSendRequest = serde_json::from_str(args)
                .map_err(|err| anyhow!("Invalid sendp2sh JSON: {}", err))?;
            let txid = execute(WalletServiceRequest::QortalSendP2sh { wallet_id, request })?;
            json!({ "txid": txid })
        }
        "redeemp2sh" => {
            let request: QortalP2shRedeemRequest = serde_json::from_str(args)
                .map_err(|err| anyhow!("Invalid redeemp2sh JSON: {}", err))?;
            let txid = execute(WalletServiceRequest::QortalRedeemP2sh { wallet_id, request })?;
            json!({ "txid": txid })
        }
        "export" => export_primary_key(wallet_id)?,
        "encryptionstatus" => json!({ "encrypted": true, "locked": false }),
        "encrypt" | "decrypt" | "unlock" => json!({ "result": "success" }),
        other => return Err(anyhow!("Unsupported Qortal wallet command: {}", other)),
    };
    Ok(serde_json::to_string(&value)?)
}

fn mnemonic_from_base64(entropy_base64: &str) -> Result<String> {
    let entropy = BASE64
        .decode(entropy_base64.trim())
        .map_err(|err| anyhow!("Invalid base64 entropy: {}", err))?;
    let mnemonic = mnemonic_from_entropy(&entropy, MnemonicLanguage::English)
        .map_err(|err| anyhow!(err.to_string()))?;
    Ok(json!({ "seedPhrase": mnemonic }).to_string())
}

fn mnemonic_from_raw_string(entropy: &str) -> Result<String> {
    let mnemonic = mnemonic_from_entropy(entropy.as_bytes(), MnemonicLanguage::English)
        .map_err(|err| anyhow!(err.to_string()))?;
    Ok(json!({ "seedPhrase": mnemonic }).to_string())
}

fn generate_seed_phrase() -> Result<String> {
    let mnemonic: String =
        serde_json::from_value(execute(WalletServiceRequest::GenerateMnemonic {
            word_count: Some(24),
            mnemonic_language: Some(MnemonicLanguage::English),
        })?)?;
    Ok(json!({ "seedPhrase": mnemonic }).to_string())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rust_litewalletjni_LiteWalletJni_configurestorage(
    mut env: JNIEnv,
    _class: JClass,
    base_dir: JString,
    passphrase: JString,
) -> jstring {
    let response = (|| {
        configure_storage(
            read_string(&mut env, base_dir)?,
            read_string(&mut env, passphrase)?,
        )
    })();
    write_string(&mut env, result_json(response))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rust_litewalletjni_LiteWalletJni_initlogging(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    write_string(&mut env, "OK".to_string())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rust_litewalletjni_LiteWalletJni_initnew(
    mut env: JNIEnv,
    _class: JClass,
    server_uri: JString,
    _params: JString,
    _sapling_output: JString,
    _sapling_spend: JString,
) -> jstring {
    let response = (|| create_wallet(read_string(&mut env, server_uri)?))();
    write_string(&mut env, result_json(response))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rust_litewalletjni_LiteWalletJni_initfromseed(
    mut env: JNIEnv,
    _class: JClass,
    server_uri: JString,
    _params: JString,
    seed: JString,
    birthday: JString,
    _sapling_output: JString,
    _sapling_spend: JString,
) -> jstring {
    let response = (|| {
        initialize_from_seed(
            read_string(&mut env, server_uri)?,
            read_string(&mut env, seed)?,
            read_string(&mut env, birthday)?,
        )
    })();
    write_string(&mut env, result_json(response))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rust_litewalletjni_LiteWalletJni_initfromb64(
    mut env: JNIEnv,
    _class: JClass,
    server_uri: JString,
    _params: JString,
    _data_base64: JString,
    _sapling_output: JString,
    _sapling_spend: JString,
) -> jstring {
    let response = (|| initialize_existing(read_string(&mut env, server_uri)?))();
    write_string(&mut env, result_json(response))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rust_litewalletjni_LiteWalletJni_save(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let marker = json!({ "format": "pirate-unified-wallet-sqlite", "version": 1 }).to_string();
    write_string(&mut env, BASE64.encode(marker))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rust_litewalletjni_LiteWalletJni_execute(
    mut env: JNIEnv,
    _class: JClass,
    command: JString,
    args: JString,
) -> jstring {
    let response = (|| {
        execute_legacy_command(
            &read_string(&mut env, command)?,
            &read_string(&mut env, args)?,
        )
    })();
    write_string(&mut env, result_json(response))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rust_litewalletjni_LiteWalletJni_getseedphrase(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    write_string(&mut env, result_json(generate_seed_phrase()))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rust_litewalletjni_LiteWalletJni_getseedphrasefromentropy(
    mut env: JNIEnv,
    _class: JClass,
    entropy: JString,
) -> jstring {
    let response = (|| mnemonic_from_raw_string(&read_string(&mut env, entropy)?))();
    write_string(&mut env, result_json(response))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rust_litewalletjni_LiteWalletJni_getseedphrasefromentropyb64(
    mut env: JNIEnv,
    _class: JClass,
    entropy_base64: JString,
) -> jstring {
    let response = (|| mnemonic_from_base64(&read_string(&mut env, entropy_base64)?))();
    write_string(&mut env, result_json(response))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rust_litewalletjni_LiteWalletJni_checkseedphrase(
    mut env: JNIEnv,
    _class: JClass,
    input: JString,
) -> jstring {
    let response = (|| {
        let input = read_string(&mut env, input)?;
        let inspection = inspect_mnemonic(&input);
        let result = if inspection.is_valid { "Ok" } else { "Error" };
        Ok(json!({ "checkSeedPhrase": result }).to_string())
    })();
    write_string(&mut env, result_json(response))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rust_litewalletjni_LiteWalletJni_invokeJson(
    mut env: JNIEnv,
    _class: JClass,
    request_json: JString,
    pretty: jboolean,
) -> jstring {
    let response = match read_string(&mut env, request_json) {
        Ok(request) => service().execute_json(&request, pretty != 0),
        Err(err) => error_json(err),
    };
    write_string(&mut env, response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pirate_core::keys::ExtendedSpendingKey;

    #[test]
    fn entropy_conversion_matches_qortal_response_shape() {
        let entropy = [7u8; 32];
        let response = mnemonic_from_base64(&BASE64.encode(entropy)).unwrap();
        let value: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(
            value["seedPhrase"]
                .as_str()
                .unwrap()
                .split_whitespace()
                .count(),
            24
        );
    }

    #[test]
    fn wallet_name_is_stable_without_exposing_the_seed() {
        let name = deterministic_wallet_name("abandon abandon abandon");
        assert_eq!(name, deterministic_wallet_name("abandon abandon abandon"));
        assert!(!name.contains("abandon"));
    }

    #[test]
    fn qortal_entropy_preserves_legacy_sapling_address() {
        let entropy = [7u8; 32];
        let mnemonic = mnemonic_from_entropy(&entropy, MnemonicLanguage::English).unwrap();
        let key = ExtendedSpendingKey::from_mnemonic(&mnemonic).unwrap();
        let address = key.to_extended_fvk().derive_address(0).encode();

        assert_eq!(
            address,
            "zs1ra3g8uphtg8ad7p8ye76pg06nr9rg5y8m5ycq40vpw4nvae6amehenaafv02g3dny9myxz7f60s"
        );
    }
}
