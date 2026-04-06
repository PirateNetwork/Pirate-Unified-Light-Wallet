use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand, ValueEnum};
use pirate_core::keys::{
    ExtendedFullViewingKey as SaplingExtendedFullViewingKey, ExtendedSpendingKey,
    OrchardExtendedFullViewingKey, OrchardExtendedSpendingKey,
};
use pirate_wallet_service::{
    AddressBalanceInfo, Balance, KeyAddressInfo, KeyExportInfo, KeyGroupInfo, Output, PendingTx,
    QortalP2shRedeemRequest, QortalP2shSendRequest, SignedTx, SyncMode, SyncStatus, TxInfo,
    WalletMeta, WalletService, WalletServiceRequest,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, Write};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Json,
    Pretty,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SyncModeArg {
    Compact,
    Deep,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AddressPoolArg {
    Sapling,
    Orchard,
    Z,
}

#[derive(Debug, Parser)]
#[command(name = "piratewallet-cli")]
#[command(about = "Pirate Unified Wallet command line interface")]
struct Cli {
    #[arg(long, value_enum, default_value = "pretty", global = true)]
    format: OutputFormat,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    BuildInfo,
    NetworkInfo,
    GenerateMnemonic {
        #[arg(long)]
        word_count: Option<u32>,
    },
    ValidateMnemonic {
        mnemonic: String,
    },
    ExecJson {
        request_json: String,
    },
    Wallet {
        #[command(subcommand)]
        command: WalletCommand,
    },
    Address {
        #[command(subcommand)]
        command: AddressCommand,
    },
    Addresses {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Balance {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Transactions {
        #[arg(long)]
        wallet_id: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    List {
        #[arg(long)]
        wallet_id: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    Lasttxid {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Height {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Notes {
        #[arg(long)]
        wallet_id: Option<String>,
        #[arg(long)]
        all: bool,
    },
    Info {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Defaultfee,
    New {
        #[arg(long)]
        wallet_id: Option<String>,
        #[arg(long)]
        key_id: Option<i64>,
        #[arg(value_enum, default_value = "sapling")]
        pool: AddressPoolArg,
    },
    Seed {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Import {
        key: String,
        birthday: u32,
        #[arg(long)]
        wallet_id: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        no_rescan: bool,
    },
    Export {
        #[arg(long)]
        wallet_id: Option<String>,
        target: Option<String>,
    },
    Clear {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Syncstatus {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Stop {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Sync {
        #[command(subcommand)]
        command: Option<SyncCommand>,
        #[arg(long)]
        wallet_id: Option<String>,
        #[arg(value_enum, default_value = "compact")]
        mode: SyncModeArg,
    },
    Send {
        #[command(subcommand)]
        command: Option<SendCommand>,
        request_json: Option<String>,
    },
    Diag {
        #[command(subcommand)]
        command: DiagCommand,
    },
}

#[derive(Debug, Subcommand)]
enum WalletCommand {
    RegistryExists,
    List,
    Active,
    Create {
        name: String,
        #[arg(long)]
        birthday: Option<u32>,
    },
    Restore {
        name: String,
        mnemonic: String,
        #[arg(long)]
        birthday: Option<u32>,
    },
    ImportViewing {
        name: String,
        #[arg(long)]
        sapling_viewing_key: Option<String>,
        #[arg(long)]
        orchard_viewing_key: Option<String>,
        #[arg(long)]
        birthday: u32,
    },
    Switch {
        wallet_id: String,
    },
    Rename {
        wallet_id: String,
        new_name: String,
    },
    SetBirthday {
        wallet_id: String,
        birthday: u32,
    },
    Delete {
        wallet_id: String,
    },
}

#[derive(Debug, Subcommand)]
enum AddressCommand {
    Current {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Next {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    List {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Balances {
        #[arg(long)]
        wallet_id: Option<String>,
        #[arg(long)]
        key_id: Option<i64>,
    },
}

#[derive(Debug, Subcommand)]
enum SyncCommand {
    Start {
        #[arg(long)]
        wallet_id: Option<String>,
        #[arg(value_enum, default_value = "compact")]
        mode: SyncModeArg,
    },
    Status {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Cancel {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Rescan {
        #[arg(long)]
        wallet_id: Option<String>,
        from_height: u32,
    },
}

#[derive(Debug, Subcommand)]
enum SendCommand {
    Build {
        wallet_id: String,
        outputs_json: String,
        #[arg(long)]
        fee: Option<u64>,
    },
    Sign {
        wallet_id: String,
        pending_json: String,
    },
    Broadcast {
        signed_json: String,
    },
}

#[derive(Debug, Subcommand)]
enum DiagCommand {
    Logs {
        wallet_id: String,
        #[arg(long)]
        limit: Option<u32>,
    },
    Checkpoint {
        wallet_id: String,
        height: u32,
    },
}

#[derive(Debug, Parser)]
#[command(name = "pirate-qortal-cli")]
#[command(about = "Qortal compatibility adapter for Pirate Unified Wallet")]
struct QortalCli {
    #[arg(long, value_enum, default_value = "pretty", global = true)]
    format: OutputFormat,

    #[command(subcommand)]
    command: QortalCommand,
}

#[derive(Debug, Subcommand)]
enum QortalCommand {
    Syncstatus {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Balance {
        #[arg(long)]
        wallet_id: Option<String>,
    },
    List {
        #[arg(long)]
        wallet_id: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    Sendp2sh {
        request_json: String,
        #[arg(long)]
        wallet_id: Option<String>,
    },
    Redeemp2sh {
        request_json: String,
        #[arg(long)]
        wallet_id: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct LegacySendRequest {
    output: Vec<LegacySendOutput>,
    fee: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct LegacySendOutput {
    address: String,
    amount: u64,
    memo: Option<String>,
}

impl From<LegacySendOutput> for Output {
    fn from(value: LegacySendOutput) -> Self {
        Output::new(value.address, value.amount, value.memo)
    }
}

pub async fn run_from_env() -> Result<()> {
    run_from_iter(std::env::args()).await
}

pub async fn run_from_iter<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    run_cli(cli).await
}

pub async fn run_qortal_from_iter<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let args_vec: Vec<std::ffi::OsString> = args.into_iter().map(Into::into).collect();
    if let Some(first) = args_vec.get(1).and_then(|s| s.to_str()) {
        match first {
            "syncstatus" | "balance" | "list" | "sendp2sh" | "redeemp2sh" => {
                let cli = QortalCli::parse_from(args_vec);
                return run_qortal_cli(cli).await;
            }
            _ => return run_cli(Cli::parse_from(args_vec)).await,
        }
    }

    run_cli(Cli::parse_from(args_vec)).await
}

async fn run_cli(cli: Cli) -> Result<()> {
    let service = WalletService::new();
    match cli.command {
        Some(command) => {
            let value = execute_command(&service, command).await?;
            print_value(&value, cli.format)?;
        }
        None => repl(&service, cli.format).await?,
    }
    Ok(())
}

async fn run_qortal_cli(cli: QortalCli) -> Result<()> {
    let service = WalletService::new();
    let value = match cli.command {
        QortalCommand::Syncstatus { wallet_id } => qortal_syncstatus(&service, wallet_id).await?,
        QortalCommand::Balance { wallet_id } => qortal_balance(&service, wallet_id).await?,
        QortalCommand::List { wallet_id, limit } => qortal_list(&service, wallet_id, limit).await?,
        QortalCommand::Sendp2sh {
            request_json,
            wallet_id,
        } => qortal_sendp2sh(&service, wallet_id, &request_json).await?,
        QortalCommand::Redeemp2sh {
            request_json,
            wallet_id,
        } => qortal_redeemp2sh(&service, wallet_id, &request_json).await?,
    };
    print_value(&value, cli.format)
}

async fn execute_command(service: &WalletService, command: Command) -> Result<Value> {
    use WalletServiceRequest as Req;

    match command {
        Command::BuildInfo => service.execute(Req::GetBuildInfo).await,
        Command::NetworkInfo => service.execute(Req::GetNetworkInfo).await,
        Command::GenerateMnemonic { word_count } => {
            service.execute(Req::GenerateMnemonic { word_count }).await
        }
        Command::ValidateMnemonic { mnemonic } => {
            service.execute(Req::ValidateMnemonic { mnemonic }).await
        }
        Command::ExecJson { request_json } => {
            let output = service.execute_json(&request_json, true);
            Ok(serde_json::from_str(&output)?)
        }
        Command::Wallet { command } => match command {
            WalletCommand::RegistryExists => service.execute(Req::WalletRegistryExists).await,
            WalletCommand::List => service.execute(Req::ListWallets).await,
            WalletCommand::Active => service.execute(Req::GetActiveWallet).await,
            WalletCommand::Create { name, birthday } => {
                service
                    .execute(Req::CreateWallet {
                        name,
                        birthday_opt: birthday,
                    })
                    .await
            }
            WalletCommand::Restore {
                name,
                mnemonic,
                birthday,
            } => {
                service
                    .execute(Req::RestoreWallet {
                        name,
                        mnemonic,
                        birthday_opt: birthday,
                    })
                    .await
            }
            WalletCommand::ImportViewing {
                name,
                sapling_viewing_key,
                orchard_viewing_key,
                birthday,
            } => {
                service
                    .execute(Req::ImportViewingWallet {
                        name,
                        sapling_viewing_key,
                        orchard_viewing_key,
                        birthday,
                    })
                    .await
            }
            WalletCommand::Switch { wallet_id } => {
                service.execute(Req::SwitchWallet { wallet_id }).await
            }
            WalletCommand::Rename {
                wallet_id,
                new_name,
            } => {
                service
                    .execute(Req::RenameWallet {
                        wallet_id,
                        new_name,
                    })
                    .await
            }
            WalletCommand::SetBirthday {
                wallet_id,
                birthday,
            } => {
                service
                    .execute(Req::SetWalletBirthdayHeight {
                        wallet_id,
                        birthday_height: birthday,
                    })
                    .await
            }
            WalletCommand::Delete { wallet_id } => {
                service.execute(Req::DeleteWallet { wallet_id }).await
            }
        },
        Command::Address { command } => match command {
            AddressCommand::Current { wallet_id } => {
                let wallet_id = resolve_wallet_id(service, wallet_id).await?;
                service
                    .execute(Req::CurrentReceiveAddress { wallet_id })
                    .await
            }
            AddressCommand::Next { wallet_id } => {
                let wallet_id = resolve_wallet_id(service, wallet_id).await?;
                service.execute(Req::NextReceiveAddress { wallet_id }).await
            }
            AddressCommand::List { wallet_id } => {
                let wallet_id = resolve_wallet_id(service, wallet_id).await?;
                let value = service.execute(Req::ListAddresses { wallet_id }).await?;
                Ok(sanitize_cli_value(value, &["label", "color_tag"]))
            }
            AddressCommand::Balances { wallet_id, key_id } => {
                let wallet_id = resolve_wallet_id(service, wallet_id).await?;
                let value = service
                    .execute(Req::ListAddressBalances { wallet_id, key_id })
                    .await?;
                Ok(sanitize_cli_value(value, &["label", "color_tag"]))
            }
        },
        Command::Addresses { wallet_id } => legacy_addresses(service, wallet_id).await,
        Command::Balance { wallet_id } => legacy_balance(service, wallet_id).await,
        Command::Transactions { wallet_id, limit } => {
            let wallet_id = resolve_wallet_id(service, wallet_id).await?;
            service
                .execute(Req::ListTransactions { wallet_id, limit })
                .await
        }
        Command::List { wallet_id, limit } => qortal_list(service, wallet_id, limit).await,
        Command::Lasttxid { wallet_id } => legacy_lasttxid(service, wallet_id).await,
        Command::Height { wallet_id } => legacy_height(service, wallet_id).await,
        Command::Notes { wallet_id, all } => {
            let wallet_id = resolve_wallet_id(service, wallet_id).await?;
            service
                .execute(Req::ListNotes {
                    wallet_id,
                    all_notes: all,
                })
                .await
        }
        Command::Info { wallet_id } => legacy_info(service, wallet_id).await,
        Command::Defaultfee => legacy_default_fee(service).await,
        Command::New {
            wallet_id,
            key_id,
            pool,
        } => legacy_new_address(service, wallet_id, key_id, pool).await,
        Command::Seed { wallet_id } => legacy_seed(service, wallet_id).await,
        Command::Import {
            key,
            birthday,
            wallet_id,
            name,
            no_rescan,
        } => legacy_import(service, key, birthday, wallet_id, name, no_rescan).await,
        Command::Export { wallet_id, target } => legacy_export(service, wallet_id, target).await,
        Command::Clear { wallet_id } => {
            let wallet_id = resolve_wallet_id(service, wallet_id).await?;
            service.execute(Req::ClearWalletState { wallet_id }).await
        }
        Command::Syncstatus { wallet_id } => qortal_syncstatus(service, wallet_id).await,
        Command::Stop { wallet_id } => {
            let wallet_id = resolve_wallet_id(service, wallet_id).await?;
            service.execute(Req::CancelSync { wallet_id }).await
        }
        Command::Sync {
            command: Some(command),
            wallet_id: _,
            mode: _,
        } => match command {
            SyncCommand::Start { wallet_id, mode } => {
                let wallet_id = resolve_wallet_id(service, wallet_id).await?;
                service
                    .execute(Req::StartSync {
                        wallet_id,
                        mode: sync_mode(mode),
                    })
                    .await
            }
            SyncCommand::Status { wallet_id } => {
                let wallet_id = resolve_wallet_id(service, wallet_id).await?;
                service.execute(Req::SyncStatus { wallet_id }).await
            }
            SyncCommand::Cancel { wallet_id } => {
                let wallet_id = resolve_wallet_id(service, wallet_id).await?;
                service.execute(Req::CancelSync { wallet_id }).await
            }
            SyncCommand::Rescan {
                wallet_id,
                from_height,
            } => {
                let wallet_id = resolve_wallet_id(service, wallet_id).await?;
                service
                    .execute(Req::Rescan {
                        wallet_id,
                        from_height,
                    })
                    .await
            }
        },
        Command::Sync {
            command: None,
            wallet_id,
            mode,
        } => {
            let wallet_id = resolve_wallet_id(service, wallet_id).await?;
            service
                .execute(Req::StartSync {
                    wallet_id,
                    mode: sync_mode(mode),
                })
                .await
        }
        Command::Send {
            command: Some(command),
            request_json: _,
        } => match command {
            SendCommand::Build {
                wallet_id,
                outputs_json,
                fee,
            } => {
                let outputs: Vec<Output> = serde_json::from_str(&outputs_json)
                    .map_err(|e| anyhow!("Invalid outputs JSON: {}", e))?;
                service
                    .execute(Req::BuildTx {
                        wallet_id,
                        outputs,
                        fee_opt: fee,
                    })
                    .await
            }
            SendCommand::Sign {
                wallet_id,
                pending_json,
            } => {
                let pending: PendingTx = serde_json::from_str(&pending_json)
                    .map_err(|e| anyhow!("Invalid pending transaction JSON: {}", e))?;
                service.execute(Req::SignTx { wallet_id, pending }).await
            }
            SendCommand::Broadcast { signed_json } => {
                let signed: SignedTx = serde_json::from_str(&signed_json)
                    .map_err(|e| anyhow!("Invalid signed transaction JSON: {}", e))?;
                service.execute(Req::BroadcastTx { signed }).await
            }
        },
        Command::Send {
            command: None,
            request_json,
        } => {
            let request_json = request_json.ok_or_else(|| {
                anyhow!("send requires either a subcommand or legacy JSON payload")
            })?;
            legacy_send(service, &request_json).await
        }
        Command::Diag { command } => match command {
            DiagCommand::Logs { wallet_id, limit } => {
                service.execute(Req::GetSyncLogs { wallet_id, limit }).await
            }
            DiagCommand::Checkpoint { wallet_id, height } => {
                service
                    .execute(Req::GetCheckpointDetails { wallet_id, height })
                    .await
            }
        },
    }
}

async fn get_wallet_meta(service: &WalletService, wallet_id: &str) -> Result<WalletMeta> {
    let wallets = service.execute(WalletServiceRequest::ListWallets).await?;
    let wallets: Vec<WalletMeta> = serde_json::from_value(wallets)?;
    wallets
        .into_iter()
        .find(|wallet| wallet.id == wallet_id)
        .ok_or_else(|| anyhow!("Wallet {} not found", wallet_id))
}

fn sync_mode(mode: SyncModeArg) -> SyncMode {
    match mode {
        SyncModeArg::Compact => SyncMode::Compact,
        SyncModeArg::Deep => SyncMode::Deep,
    }
}

async fn resolve_wallet_id(service: &WalletService, wallet_id: Option<String>) -> Result<String> {
    if let Some(wallet_id) = wallet_id {
        return Ok(wallet_id);
    }

    let active_wallet = service
        .execute(WalletServiceRequest::GetActiveWallet)
        .await?;
    let active_wallet: Option<String> = serde_json::from_value(active_wallet)?;
    active_wallet.ok_or_else(|| anyhow!("No active wallet selected"))
}

fn sanitize_cli_value(mut value: Value, hidden_fields: &[&str]) -> Value {
    strip_hidden_fields(&mut value, hidden_fields);
    value
}

fn strip_hidden_fields(value: &mut Value, hidden_fields: &[&str]) {
    match value {
        Value::Array(entries) => {
            for entry in entries {
                strip_hidden_fields(entry, hidden_fields);
            }
        }
        Value::Object(map) => {
            for field in hidden_fields {
                map.remove(*field);
            }
            for entry in map.values_mut() {
                strip_hidden_fields(entry, hidden_fields);
            }
        }
        _ => {}
    }
}

async fn legacy_addresses(service: &WalletService, wallet_id: Option<String>) -> Result<Value> {
    let wallet_id = resolve_wallet_id(service, wallet_id).await?;
    let addresses = service
        .execute(WalletServiceRequest::ListAddresses { wallet_id })
        .await?;
    let addresses: Vec<Value> = serde_json::from_value(addresses)?;

    let z_addresses: Vec<String> = addresses
        .iter()
        .filter_map(|entry| {
            entry
                .get("address")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect();

    Ok(json!({
        "z_addresses": z_addresses,
        "t_addresses": [],
    }))
}

async fn legacy_balance(service: &WalletService, wallet_id: Option<String>) -> Result<Value> {
    qortal_balance(service, wallet_id).await
}

async fn legacy_lasttxid(service: &WalletService, wallet_id: Option<String>) -> Result<Value> {
    let wallet_id = resolve_wallet_id(service, wallet_id).await?;
    let txs = service
        .execute(WalletServiceRequest::ListTransactions {
            wallet_id,
            limit: Some(1),
        })
        .await?;
    let txs: Vec<TxInfo> = serde_json::from_value(txs)?;
    Ok(json!({
        "last_txid": txs.into_iter().next().map(|tx| tx.txid),
    }))
}

async fn legacy_height(service: &WalletService, wallet_id: Option<String>) -> Result<Value> {
    let wallet_id = resolve_wallet_id(service, wallet_id).await?;
    let status = service
        .execute(WalletServiceRequest::SyncStatus { wallet_id })
        .await?;
    let status: SyncStatus = serde_json::from_value(status)?;
    Ok(json!({ "height": status.local_height }))
}

async fn legacy_info(service: &WalletService, _wallet_id: Option<String>) -> Result<Value> {
    let build = service.execute(WalletServiceRequest::GetBuildInfo).await?;
    let network = service
        .execute(WalletServiceRequest::GetNetworkInfo)
        .await?;
    Ok(json!({
        "build": build,
        "network": network,
    }))
}

async fn legacy_default_fee(service: &WalletService) -> Result<Value> {
    let fee = service.execute(WalletServiceRequest::GetFeeInfo).await?;
    let fee = fee
        .get("default_fee")
        .cloned()
        .ok_or_else(|| anyhow!("Fee info missing default_fee"))?;
    Ok(json!({ "defaultfee": fee }))
}

async fn legacy_seed(service: &WalletService, wallet_id: Option<String>) -> Result<Value> {
    let wallet_id = resolve_wallet_id(service, wallet_id).await?;
    let seed = pirate_wallet_service::export_seed_raw(wallet_id.clone())?;
    let wallet = get_wallet_meta(service, &wallet_id).await?;
    Ok(json!({
        "seed": seed,
        "birthday": wallet.birthday_height,
    }))
}

async fn legacy_import(
    service: &WalletService,
    key: String,
    birthday: u32,
    wallet_id: Option<String>,
    name: Option<String>,
    no_rescan: bool,
) -> Result<Value> {
    if ExtendedSpendingKey::from_bech32_any(&key).is_ok() {
        let wallet_id = resolve_wallet_id(service, wallet_id).await?;
        let key_id = pirate_wallet_service::import_spending_key(
            wallet_id.clone(),
            Some(key),
            None,
            None,
            birthday,
        )?;
        if !no_rescan {
            service
                .execute(WalletServiceRequest::Rescan {
                    wallet_id: wallet_id.clone(),
                    from_height: birthday,
                })
                .await?;
        }
        return Ok(json!({ "key_id": key_id }));
    }

    if OrchardExtendedSpendingKey::from_bech32_any(&key).is_ok() {
        let wallet_id = resolve_wallet_id(service, wallet_id).await?;
        let key_id = pirate_wallet_service::import_spending_key(
            wallet_id.clone(),
            None,
            Some(key),
            None,
            birthday,
        )?;
        if !no_rescan {
            service
                .execute(WalletServiceRequest::Rescan {
                    wallet_id: wallet_id.clone(),
                    from_height: birthday,
                })
                .await?;
        }
        return Ok(json!({ "key_id": key_id }));
    }

    if SaplingExtendedFullViewingKey::from_xfvk_bech32_any(&key).is_ok() {
        let name =
            name.ok_or_else(|| anyhow!("--name is required when importing a viewing key"))?;
        let wallet_id =
            pirate_wallet_service::import_viewing_wallet(name, Some(key), None, birthday)?;
        if !no_rescan {
            service
                .execute(WalletServiceRequest::StartSync {
                    wallet_id: wallet_id.clone(),
                    mode: SyncMode::Compact,
                })
                .await?;
        }
        return Ok(json!({ "wallet_id": wallet_id }));
    }

    if OrchardExtendedFullViewingKey::from_bech32_any(&key).is_ok() {
        let name =
            name.ok_or_else(|| anyhow!("--name is required when importing a viewing key"))?;
        let wallet_id =
            pirate_wallet_service::import_viewing_wallet(name, None, Some(key), birthday)?;
        if !no_rescan {
            service
                .execute(WalletServiceRequest::StartSync {
                    wallet_id: wallet_id.clone(),
                    mode: SyncMode::Compact,
                })
                .await?;
        }
        return Ok(json!({ "wallet_id": wallet_id }));
    }

    Err(anyhow!(
        "Key was not recognized as a Sapling/Orchard spending key or viewing key"
    ))
}

async fn legacy_export(
    service: &WalletService,
    wallet_id: Option<String>,
    target: Option<String>,
) -> Result<Value> {
    let wallet_id = resolve_wallet_id(service, wallet_id).await?;
    let key_groups = pirate_wallet_service::list_key_groups(wallet_id.clone())?;

    if let Some(target) = target {
        let key_id = if let Ok(key_id) = target.parse::<i64>() {
            key_id
        } else {
            resolve_key_id_for_address(&wallet_id, &key_groups, &target)?
        };
        let exported = pirate_wallet_service::export_key_group_keys(wallet_id, key_id)?;
        return Ok(serde_json::to_value(exported)?);
    }

    let exported: Vec<KeyExportInfo> = key_groups
        .iter()
        .map(|group| pirate_wallet_service::export_key_group_keys(wallet_id.clone(), group.id))
        .collect::<Result<_>>()?;
    Ok(serde_json::to_value(exported)?)
}

fn resolve_key_id_for_address(
    wallet_id: &str,
    key_groups: &[KeyGroupInfo],
    address: &str,
) -> Result<i64> {
    for group in key_groups {
        let addresses =
            pirate_wallet_service::list_addresses_for_key(wallet_id.to_string(), group.id)?;
        if addresses
            .iter()
            .any(|entry: &KeyAddressInfo| entry.address == address)
        {
            return Ok(group.id);
        }
    }
    Err(anyhow!("Address not found in wallet key groups"))
}

async fn legacy_new_address(
    service: &WalletService,
    wallet_id: Option<String>,
    key_id: Option<i64>,
    pool: AddressPoolArg,
) -> Result<Value> {
    let wallet_id = resolve_wallet_id(service, wallet_id).await?;
    let use_orchard = matches!(pool, AddressPoolArg::Orchard);
    let pool_label = match pool {
        AddressPoolArg::Sapling | AddressPoolArg::Z => "z",
        AddressPoolArg::Orchard => "orchard",
    };

    let address = if let Some(key_id) = key_id {
        pirate_wallet_service::generate_address_for_key(wallet_id.clone(), key_id, use_orchard)?
    } else if matches!(pool, AddressPoolArg::Sapling | AddressPoolArg::Z) {
        pirate_wallet_service::next_receive_address(wallet_id.clone())?
    } else {
        let key_groups = pirate_wallet_service::list_key_groups(wallet_id.clone())?;
        let group = key_groups
            .into_iter()
            .find(|group| group.spendable && group.has_orchard)
            .ok_or_else(|| anyhow!("No Orchard-capable key group found"))?;
        pirate_wallet_service::generate_address_for_key(wallet_id.clone(), group.id, true)?
    };

    Ok(json!({
        "pool": pool_label,
        "address": address,
    }))
}

async fn legacy_send(service: &WalletService, request_json: &str) -> Result<Value> {
    let request: LegacySendRequest =
        serde_json::from_str(request_json).map_err(|e| anyhow!("Invalid send JSON: {}", e))?;
    let wallet_id = resolve_wallet_id(service, None).await?;
    let outputs: Vec<Output> = request.output.into_iter().map(Into::into).collect();
    let pending = service
        .execute(WalletServiceRequest::BuildTx {
            wallet_id: wallet_id.clone(),
            outputs,
            fee_opt: request.fee,
        })
        .await?;
    let pending: PendingTx = serde_json::from_value(pending)?;
    let signed = service
        .execute(WalletServiceRequest::SignTx { wallet_id, pending })
        .await?;
    let signed: SignedTx = serde_json::from_value(signed)?;
    let txid = service
        .execute(WalletServiceRequest::BroadcastTx { signed })
        .await?;
    Ok(json!({ "txid": txid }))
}

async fn qortal_syncstatus(service: &WalletService, wallet_id: Option<String>) -> Result<Value> {
    let wallet_id = resolve_wallet_id(service, wallet_id).await?;
    let status = service
        .execute(WalletServiceRequest::SyncStatus {
            wallet_id: wallet_id.clone(),
        })
        .await?;
    let status: SyncStatus = serde_json::from_value(status)?;
    Ok(format_qortal_syncstatus(wallet_id, &status))
}

async fn qortal_balance(service: &WalletService, wallet_id: Option<String>) -> Result<Value> {
    let wallet_id = resolve_wallet_id(service, wallet_id).await?;
    let balance = service
        .execute(WalletServiceRequest::GetBalance {
            wallet_id: wallet_id.clone(),
        })
        .await?;
    let balance: Balance = serde_json::from_value(balance)?;
    let address_balances = service
        .execute(WalletServiceRequest::ListAddressBalances {
            wallet_id,
            key_id: None,
        })
        .await?;
    let address_balances: Vec<AddressBalanceInfo> = serde_json::from_value(address_balances)?;

    Ok(format_qortal_balance(&balance, address_balances))
}

async fn qortal_list(
    service: &WalletService,
    wallet_id: Option<String>,
    limit: Option<u32>,
) -> Result<Value> {
    let wallet_id = resolve_wallet_id(service, wallet_id).await?;
    let txs = service
        .execute(WalletServiceRequest::ListTransactions { wallet_id, limit })
        .await?;
    let txs: Vec<TxInfo> = serde_json::from_value(txs)?;

    Ok(format_qortal_list(txs))
}

async fn qortal_sendp2sh(
    service: &WalletService,
    wallet_id: Option<String>,
    request_json: &str,
) -> Result<Value> {
    let wallet_id = resolve_wallet_id(service, wallet_id).await?;
    let request: QortalP2shSendRequest =
        serde_json::from_str(request_json).map_err(|e| anyhow!("Invalid sendp2sh JSON: {}", e))?;
    let txid = pirate_wallet_service::qortal_send_p2sh(wallet_id, request).await?;
    Ok(format_qortal_txid(txid))
}

async fn qortal_redeemp2sh(
    service: &WalletService,
    wallet_id: Option<String>,
    request_json: &str,
) -> Result<Value> {
    let wallet_id = resolve_wallet_id(service, wallet_id).await?;
    let request: QortalP2shRedeemRequest = serde_json::from_str(request_json)
        .map_err(|e| anyhow!("Invalid redeemp2sh JSON: {}", e))?;
    let txid = pirate_wallet_service::qortal_redeem_p2sh(wallet_id, request).await?;
    Ok(format_qortal_txid(txid))
}

fn format_qortal_syncstatus(wallet_id: String, status: &SyncStatus) -> Value {
    if status.is_syncing() {
        json!({
            "sync_id": wallet_id,
            "in_progress": true,
            "last_error": null,
            "start_block": status.local_height,
            "end_block": status.target_height,
            "synced_blocks": status.local_height,
            "trial_decryptions_blocks": 0,
            "txn_scan_blocks": 0,
            "total_blocks": status.target_height,
            "batch_num": 0,
            "batch_total": 0,
        })
    } else {
        json!({
            "sync_id": wallet_id,
            "in_progress": false,
            "last_error": null,
            "scanned_height": status.local_height,
        })
    }
}

fn format_qortal_balance(balance: &Balance, address_balances: Vec<AddressBalanceInfo>) -> Value {
    let z_addresses: Vec<Value> = address_balances
        .into_iter()
        .map(|entry| {
            json!({
                "address": entry.address,
                "zbalance": entry.balance,
                "verified_zbalance": entry.spendable,
                "spendable_zbalance": entry.spendable,
                "unverified_zbalance": entry.balance.saturating_sub(entry.spendable),
            })
        })
        .collect();

    json!({
        "zbalance": balance.total,
        "verified_zbalance": balance.spendable,
        "spendable_zbalance": balance.spendable,
        "unverified_zbalance": balance.total.saturating_sub(balance.spendable),
        "tbalance": 0,
        "z_addresses": z_addresses,
        "t_addresses": [],
    })
}

fn format_qortal_list(txs: Vec<TxInfo>) -> Value {
    Value::Array(
        txs.into_iter()
            .map(|tx| {
                let mut entry = json!({
                    "txid": tx.txid,
                    "block_height": tx.height,
                    "datetime": tx.timestamp,
                    "amount": tx.amount,
                    "fee": tx.fee,
                    "memo": tx.memo,
                    "incoming_metadata": [],
                    "outgoing_metadata": [],
                    "incoming_metadata_change": [],
                    "outgoing_metadata_change": [],
                });
                if !tx.confirmed {
                    entry["unconfirmed"] = Value::Bool(true);
                }
                entry
            })
            .collect(),
    )
}

fn format_qortal_txid(txid: String) -> Value {
    json!({ "txid": txid })
}

fn print_value(value: &Value, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string(value)?),
        OutputFormat::Pretty => println!("{}", serde_json::to_string_pretty(value)?),
    }
    Ok(())
}

async fn repl(service: &WalletService, format: OutputFormat) -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    loop {
        write!(stdout, "piratewallet> ")?;
        stdout.flush()?;

        let mut line = String::new();
        if stdin.read_line(&mut line)? == 0 {
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if matches!(line, "quit" | "exit") {
            break;
        }
        if line == "help" {
            println!("Type any non-interactive command. Example: wallet list");
            continue;
        }

        let Some(mut args) = shlex::split(line) else {
            eprintln!("Could not parse command line");
            continue;
        };
        args.insert(0, "piratewallet-cli".to_string());

        match Cli::try_parse_from(args) {
            Ok(parsed) => {
                if let Some(command) = parsed.command {
                    match execute_command(service, command).await {
                        Ok(value) => {
                            let _ = print_value(&value, format);
                        }
                        Err(err) => {
                            eprintln!("{}", err);
                        }
                    }
                }
            }
            Err(err) => eprintln!("{}", err),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        format_qortal_balance, format_qortal_list, format_qortal_syncstatus, format_qortal_txid,
        sanitize_cli_value, QortalCli, QortalCommand,
    };
    use clap::Parser;
    use pirate_wallet_service::{
        AddressBalanceInfo, Balance, QortalP2shRedeemRequest, QortalP2shSendRequest, SyncStage,
        SyncStatus, TxInfo,
    };
    use serde_json::json;

    #[test]
    fn qortal_sendp2sh_command_parses_json_arg() {
        let parsed = QortalCli::parse_from([
            "pirate-qortal-cli",
            "sendp2sh",
            "{\"input\":\"zs1test\",\"output\":[],\"script\":\"111\",\"fee\":10000}",
        ]);
        match parsed.command {
            QortalCommand::Sendp2sh { request_json, .. } => {
                assert!(request_json.contains("\"script\""));
            }
            _ => panic!("expected sendp2sh command"),
        }
    }

    #[test]
    fn qortal_redeemp2sh_command_parses_json_arg() {
        let parsed = QortalCli::parse_from([
            "pirate-qortal-cli",
            "redeemp2sh",
            "{\"input\":\"t1test\",\"output\":[],\"fee\":10000,\"script\":\"111\",\"txid\":\"222\",\"locktime\":0,\"secret\":\"\",\"privkey\":\"333\"}",
        ]);
        match parsed.command {
            QortalCommand::Redeemp2sh { request_json, .. } => {
                assert!(request_json.contains("\"locktime\":0"));
            }
            _ => panic!("expected redeemp2sh command"),
        }
    }

    #[test]
    fn qortal_sendp2sh_request_schema_deserializes_expected_fields() {
        let request: QortalP2shSendRequest = serde_json::from_value(json!({
            "input": "zs1source",
            "output": [],
            "script": "3vQB7B6MrGQZaxCuFg4oh",
            "fee": 10000
        }))
        .expect("sendp2sh schema should deserialize");

        assert_eq!(request.input, "zs1source");
        assert_eq!(request.script, "3vQB7B6MrGQZaxCuFg4oh");
        assert_eq!(request.fee, 10000);
    }

    #[test]
    fn qortal_redeemp2sh_request_schema_deserializes_expected_fields() {
        let request: QortalP2shRedeemRequest = serde_json::from_value(json!({
            "input": "t1p2sh",
            "output": [],
            "fee": 10000,
            "script": "3vQB7B6MrGQZaxCuFg4oh",
            "txid": "4vJ9JU1bJJE96FWSJKv5",
            "locktime": 0,
            "secret": "",
            "privkey": "5HueCGU8rMjxEXxiPuD5"
        }))
        .expect("redeemp2sh schema should deserialize");

        assert_eq!(request.script, "3vQB7B6MrGQZaxCuFg4oh");
        assert_eq!(request.txid, "4vJ9JU1bJJE96FWSJKv5");
        assert_eq!(request.locktime, 0);
        assert_eq!(request.secret, "");
        assert_eq!(request.privkey, "5HueCGU8rMjxEXxiPuD5");
    }

    #[test]
    fn qortal_syncstatus_schema_when_syncing_matches_reference_contract() {
        let status = SyncStatus {
            local_height: 120,
            target_height: 240,
            percent: 50.0,
            eta: Some(125),
            stage: SyncStage::Notes,
            last_checkpoint: Some(96),
            blocks_per_second: 4.5,
            notes_decrypted: 42,
            last_batch_ms: 900,
        };

        let value = format_qortal_syncstatus("wallet-1".to_string(), &status);
        let object = value.as_object().expect("syncstatus must be an object");

        for key in [
            "sync_id",
            "in_progress",
            "last_error",
            "start_block",
            "end_block",
            "synced_blocks",
            "trial_decryptions_blocks",
            "txn_scan_blocks",
            "total_blocks",
            "batch_num",
            "batch_total",
        ] {
            assert!(object.contains_key(key), "missing key {}", key);
        }

        assert!(!object.contains_key("scanned_height"));
        assert_eq!(
            object.get("in_progress").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn qortal_syncstatus_schema_when_not_syncing_matches_reference_contract() {
        let status = SyncStatus {
            local_height: 240,
            target_height: 240,
            percent: 100.0,
            eta: None,
            stage: SyncStage::Verify,
            last_checkpoint: Some(240),
            blocks_per_second: 0.0,
            notes_decrypted: 0,
            last_batch_ms: 0,
        };

        let value = format_qortal_syncstatus("wallet-1".to_string(), &status);
        let object = value.as_object().expect("syncstatus must be an object");

        for key in ["sync_id", "in_progress", "last_error", "scanned_height"] {
            assert!(object.contains_key(key), "missing key {}", key);
        }

        for forbidden in [
            "start_block",
            "end_block",
            "synced_blocks",
            "trial_decryptions_blocks",
            "txn_scan_blocks",
            "total_blocks",
            "batch_num",
            "batch_total",
        ] {
            assert!(
                !object.contains_key(forbidden),
                "unexpected key {} when not syncing",
                forbidden
            );
        }
    }

    #[test]
    fn qortal_balance_schema_matches_reference_contract() {
        let balance = Balance {
            total: 25_000,
            spendable: 20_000,
            pending: 5_000,
        };
        let address_balances = vec![AddressBalanceInfo {
            address: "zs1recipient".to_string(),
            balance: 25_000,
            spendable: 20_000,
            pending: 5_000,
            key_id: Some(7),
            address_id: 11,
            label: Some("Main".to_string()),
            created_at: 1_710_000_000,
            color_tag: pirate_wallet_service::AddressBookColorTag::None,
            diversifier_index: 0,
        }];

        let value = format_qortal_balance(&balance, address_balances);
        let object = value.as_object().expect("balance must be an object");

        for key in [
            "zbalance",
            "verified_zbalance",
            "spendable_zbalance",
            "unverified_zbalance",
            "tbalance",
            "z_addresses",
            "t_addresses",
        ] {
            assert!(object.contains_key(key), "missing key {}", key);
        }

        assert_eq!(object.get("tbalance").and_then(|v| v.as_i64()), Some(0));
        assert_eq!(
            object
                .get("t_addresses")
                .and_then(|v| v.as_array())
                .map(|v| v.len()),
            Some(0)
        );
    }

    #[test]
    fn qortal_list_schema_matches_reference_contract() {
        let txs = vec![
            TxInfo {
                txid: "confirmed".to_string(),
                height: Some(123),
                timestamp: 1_710_000_100,
                amount: 50_000,
                fee: 10_000,
                memo: Some("hello".to_string()),
                confirmed: true,
            },
            TxInfo {
                txid: "unconfirmed".to_string(),
                height: None,
                timestamp: 1_710_000_200,
                amount: -25_000,
                fee: 10_000,
                memo: None,
                confirmed: false,
            },
        ];

        let value = format_qortal_list(txs);
        let array = value.as_array().expect("list must be an array");
        assert_eq!(array.len(), 2);

        let confirmed = array[0].as_object().expect("entry must be an object");
        for key in [
            "txid",
            "block_height",
            "datetime",
            "amount",
            "fee",
            "memo",
            "incoming_metadata",
            "outgoing_metadata",
            "incoming_metadata_change",
            "outgoing_metadata_change",
        ] {
            assert!(confirmed.contains_key(key), "missing key {}", key);
        }
        assert!(!confirmed.contains_key("unconfirmed"));

        let unconfirmed = array[1].as_object().expect("entry must be an object");
        assert_eq!(
            unconfirmed
                .get("unconfirmed")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn qortal_sendp2sh_and_redeemp2sh_output_schema_is_txid_only() {
        let send = format_qortal_txid("sendtx".to_string());
        let redeem = format_qortal_txid("redeemtx".to_string());

        let send_object = send.as_object().expect("send result must be an object");
        let redeem_object = redeem.as_object().expect("redeem result must be an object");

        assert_eq!(send_object.len(), 1);
        assert_eq!(redeem_object.len(), 1);
        assert_eq!(
            send_object.get("txid").and_then(|value| value.as_str()),
            Some("sendtx")
        );
        assert_eq!(
            redeem_object.get("txid").and_then(|value| value.as_str()),
            Some("redeemtx")
        );
    }

    #[test]
    fn sanitize_cli_value_strips_frontend_local_fields() {
        let value = json!([{
            "address": "zs1recipient",
            "label": "Main",
            "color_tag": "Blue",
            "nested": {
                "label": "Inner",
                "color_tag": "Red",
                "keep": true
            }
        }]);

        let sanitized = sanitize_cli_value(value, &["label", "color_tag"]);
        let entry = sanitized[0].as_object().expect("sanitized entry");
        assert!(!entry.contains_key("label"));
        assert!(!entry.contains_key("color_tag"));

        let nested = entry
            .get("nested")
            .and_then(|value| value.as_object())
            .expect("nested object");
        assert!(!nested.contains_key("label"));
        assert!(!nested.contains_key("color_tag"));
        assert_eq!(
            nested.get("keep").and_then(|value| value.as_bool()),
            Some(true)
        );
    }
}
