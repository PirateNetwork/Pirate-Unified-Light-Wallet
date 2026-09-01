use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, to_value, Value};

pub use crate::{
    AddressBalanceInfo, AddressBookColorTag, AddressBookEntryFfi, AddressInfo, AddressValidation,
    AddressedDeposit, Balance, BuildInfo, CheckpointInfo, ConsensusBranchValidation,
    DepositAddressScope, EndpointPoolDiagnostics, FeeInfo, KeyExportInfo, KeyGroupInfo,
    KeyTypeInfo, LightdEndpoint, NodeTestResult, NoteInfo, Output, PaymentDisclosure,
    PaymentDisclosureVerification, PendingTx, QortalP2shRedeemRequest, QortalP2shSendRequest,
    QortalSendRequest, SeedExportWarnings, ShieldedPoolBalances, SignedTx, SpendabilityStatus,
    SyncLogEntryFfi, SyncMode, SyncStatus, TransactionCursor, TransactionDetails, TransactionPage,
    TransactionRecipient, TunnelMode, TxInfo, VerifiedSpendingKeyImport, VerifiedSpendingKeyPool,
    WalletId, WalletMeta, WalletSigningStatus, WatchOnlyBannerInfo, WatchOnlyCapabilitiesInfo,
};
pub use pirate_core::{MnemonicInspection, MnemonicLanguage};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum WalletServiceRequest {
    GetBuildInfo,
    WalletRegistryExists,
    ListWallets,
    GetActiveWallet,
    CreateWallet {
        name: String,
        birthday_opt: Option<u32>,
        mnemonic_language: Option<MnemonicLanguage>,
    },
    RestoreWallet {
        name: String,
        mnemonic: String,
        birthday_opt: Option<u32>,
        mnemonic_language: Option<MnemonicLanguage>,
    },
    ImportViewingWallet {
        name: String,
        sapling_viewing_key: Option<String>,
        ironwood_viewing_key: Option<String>,
        birthday: u32,
    },
    SwitchWallet {
        wallet_id: WalletId,
    },
    RenameWallet {
        wallet_id: WalletId,
        new_name: String,
    },
    SetWalletBirthdayHeight {
        wallet_id: WalletId,
        birthday_height: u32,
    },
    DeleteWallet {
        wallet_id: WalletId,
    },
    ConfigureWalletStorage {
        base_dir: String,
        passphrase: String,
    },
    SetAppPassphrase {
        passphrase: String,
    },
    HasAppPassphrase,
    VerifyAppPassphrase {
        passphrase: String,
    },
    UnlockApp {
        passphrase: String,
    },
    ChangeAppPassphrase {
        current_passphrase: String,
        new_passphrase: String,
    },
    ChangeAppPassphraseWithCached {
        new_passphrase: String,
    },
    CurrentReceiveAddress {
        wallet_id: WalletId,
    },
    NextReceiveAddress {
        wallet_id: WalletId,
    },
    LabelAddress {
        wallet_id: WalletId,
        address: String,
        label: String,
    },
    SetAddressColorTag {
        wallet_id: WalletId,
        address: String,
        color_tag: AddressBookColorTag,
    },
    ListAddresses {
        wallet_id: WalletId,
    },
    ListAddressBalances {
        wallet_id: WalletId,
        key_id: Option<i64>,
    },
    GetBalance {
        wallet_id: WalletId,
    },
    GetShieldedPoolBalances {
        wallet_id: WalletId,
    },
    GetFeeInfo,
    GetAutoConsolidationThreshold,
    GetAutoConsolidationCandidateCount {
        wallet_id: WalletId,
    },
    GetSpendabilityStatus {
        wallet_id: WalletId,
    },
    EnableWalletSigningProtection {
        wallet_id: WalletId,
        session_credential: String,
    },
    UnlockWalletSigning {
        wallet_id: WalletId,
        session_credential: String,
    },
    LockWalletSigning {
        wallet_id: WalletId,
    },
    LockAllWalletSigning,
    GetWalletSigningStatus {
        wallet_id: WalletId,
    },
    ListKeyGroups {
        wallet_id: WalletId,
    },
    ExportKeyGroupKeys {
        wallet_id: WalletId,
        key_id: i64,
    },
    ImportSpendingKey {
        wallet_id: WalletId,
        sapling_key: Option<String>,
        ironwood_key: Option<String>,
        label: Option<String>,
        birthday_height: u32,
    },
    ImportSpendingKeyVerified {
        wallet_id: WalletId,
        pool: VerifiedSpendingKeyPool,
        spending_key: String,
        expected_address: String,
        address_index: u32,
        label: Option<String>,
        birthday_height: u32,
    },
    ExportSeedRaw {
        wallet_id: WalletId,
        mnemonic_language: Option<MnemonicLanguage>,
    },
    ListTransactions {
        wallet_id: WalletId,
        limit: Option<u32>,
    },
    ListIncomingDeposits {
        wallet_id: WalletId,
        limit: Option<u32>,
    },
    ListTransactionsPage {
        wallet_id: WalletId,
        cursor: Option<TransactionCursor>,
        page_size: u32,
    },
    QortalSyncStatus {
        wallet_id: WalletId,
    },
    QortalBalance {
        wallet_id: WalletId,
    },
    QortalListTransactions {
        wallet_id: WalletId,
        limit: Option<u32>,
    },
    QortalSend {
        wallet_id: WalletId,
        request: QortalSendRequest,
    },
    QortalSendP2sh {
        wallet_id: WalletId,
        request: QortalP2shSendRequest,
    },
    QortalRedeemP2sh {
        wallet_id: WalletId,
        request: QortalP2shRedeemRequest,
    },
    ListNotes {
        wallet_id: WalletId,
        all_notes: bool,
    },
    ClearWalletState {
        wallet_id: WalletId,
    },
    FetchTransactionMemo {
        wallet_id: WalletId,
        txid: String,
        output_index: Option<u32>,
    },
    GetTransactionDetails {
        wallet_id: WalletId,
        txid: String,
    },
    ExportPaymentDisclosures {
        wallet_id: WalletId,
        txid: String,
    },
    ExportSaplingPaymentDisclosure {
        wallet_id: WalletId,
        txid: String,
        output_index: u32,
    },
    ExportIronwoodPaymentDisclosure {
        wallet_id: WalletId,
        txid: String,
        action_index: u32,
    },
    VerifyPaymentDisclosure {
        wallet_id: WalletId,
        disclosure: String,
    },
    ListAddressBook {
        wallet_id: WalletId,
    },
    AddAddressBookEntry {
        wallet_id: WalletId,
        address: String,
        label: String,
        notes: Option<String>,
        color_tag: AddressBookColorTag,
    },
    UpdateAddressBookEntry {
        wallet_id: WalletId,
        id: i64,
        label: Option<String>,
        notes: Option<String>,
        color_tag: Option<AddressBookColorTag>,
        is_favorite: Option<bool>,
    },
    DeleteAddressBookEntry {
        wallet_id: WalletId,
        id: i64,
    },
    ToggleAddressBookFavorite {
        wallet_id: WalletId,
        id: i64,
    },
    MarkAddressUsed {
        wallet_id: WalletId,
        address: String,
    },
    GetLabelForAddress {
        wallet_id: WalletId,
        address: String,
    },
    AddressExistsInBook {
        wallet_id: WalletId,
        address: String,
    },
    GetAddressBookCount {
        wallet_id: WalletId,
    },
    GetAddressBookEntry {
        wallet_id: WalletId,
        id: i64,
    },
    GetAddressBookEntryByAddress {
        wallet_id: WalletId,
        address: String,
    },
    SearchAddressBook {
        wallet_id: WalletId,
        query: String,
    },
    GetAddressBookFavorites {
        wallet_id: WalletId,
    },
    GetRecentlyUsedAddresses {
        wallet_id: WalletId,
        limit: u32,
    },
    IsValidShieldedAddress {
        address: String,
    },
    ValidateAddress {
        address: String,
    },
    GetLightdEndpoint {
        wallet_id: WalletId,
    },
    GetLightdEndpointConfig {
        wallet_id: WalletId,
    },
    GetLightdEndpointPoolDiagnostics {
        wallet_id: WalletId,
    },
    SetLightdEndpoint {
        wallet_id: WalletId,
        url: String,
        tls_pin_opt: Option<String>,
    },
    SetLightdEndpointPool {
        wallet_id: WalletId,
        url: String,
        tls_pin_opt: Option<String>,
        failover_endpoints: Vec<String>,
    },
    GetTunnel,
    SetTunnel {
        mode: TunnelMode,
    },
    BootstrapTunnel {
        mode: TunnelMode,
    },
    ShutdownTransport,
    SetTorBridgeSettings {
        use_bridges: bool,
        fallback_to_bridges: bool,
        transport: String,
        bridge_lines: Vec<String>,
        transport_path: Option<String>,
    },
    GetTorStatus,
    RotateTorExit,
    FetchExternalText {
        url: String,
        accept: Option<String>,
        user_agent: Option<String>,
    },
    FetchExternalBytes {
        url: String,
        accept: Option<String>,
        user_agent: Option<String>,
    },
    DownloadExternalToFile {
        url: String,
        destination_path: String,
        accept: Option<String>,
        user_agent: Option<String>,
    },
    TestNode {
        url: String,
        tls_pin: Option<String>,
    },
    StartSync {
        wallet_id: WalletId,
        mode: SyncMode,
    },
    SyncStatus {
        wallet_id: WalletId,
    },
    CancelSync {
        wallet_id: WalletId,
    },
    Rescan {
        wallet_id: WalletId,
        from_height: u32,
    },
    BuildTx {
        wallet_id: WalletId,
        outputs: Vec<Output>,
        #[serde(default, with = "crate::models::amount_json::opt_u64")]
        fee_opt: Option<u64>,
    },
    SignTx {
        wallet_id: WalletId,
        pending: PendingTx,
    },
    BroadcastTx {
        #[serde(default)]
        wallet_id: Option<WalletId>,
        signed: SignedTx,
    },
    StartSeedExport {
        wallet_id: WalletId,
    },
    AcknowledgeSeedWarning,
    CompleteSeedBiometric {
        success: bool,
    },
    SkipSeedBiometric,
    ExportSeedWithPassphrase {
        wallet_id: WalletId,
        passphrase: String,
        mnemonic_language: Option<MnemonicLanguage>,
    },
    ExportSeedWithCachedPassphrase {
        wallet_id: WalletId,
        mnemonic_language: Option<MnemonicLanguage>,
    },
    CancelSeedExport,
    GetSeedExportState,
    GetSeedExportWarnings,
    ExportSaplingViewingKey {
        wallet_id: WalletId,
    },
    ExportIronwoodViewingKey {
        wallet_id: WalletId,
    },
    ExportSaplingViewingKeySecure {
        wallet_id: WalletId,
    },
    ImportSaplingViewingKeyAsWatchOnly {
        name: String,
        sapling_viewing_key: String,
        birthday_height: u32,
    },
    GetWatchOnlyCapabilities {
        wallet_id: WalletId,
    },
    GetWatchOnlyBanner {
        wallet_id: WalletId,
    },
    GetIvkClipboardRemaining,
    SetPanicPin {
        pin: String,
    },
    HasPanicPin,
    VerifyPanicPin {
        pin: String,
    },
    IsDecoyMode,
    GetVaultMode,
    ClearPanicPin,
    SetDuressPassphrase {
        custom_passphrase: Option<String>,
    },
    HasDuressPassphrase,
    ClearDuressPassphrase,
    VerifyDuressPassphrase {
        passphrase: String,
    },
    SetDecoyWalletName {
        name: String,
    },
    ExitDecoyMode {
        passphrase: String,
    },
    SetDebugLoggingEnabled {
        enabled: bool,
    },
    GetDebugLoggingEnabled,
    ClearDebugLogs,
    GetSyncLogs {
        wallet_id: WalletId,
        limit: Option<u32>,
    },
    GetCheckpointDetails {
        wallet_id: WalletId,
        height: u32,
    },
    ValidateConsensusBranch {
        wallet_id: WalletId,
    },
    GenerateMnemonic {
        word_count: Option<u32>,
        mnemonic_language: Option<MnemonicLanguage>,
    },
    ValidateMnemonic {
        mnemonic: String,
        mnemonic_language: Option<MnemonicLanguage>,
    },
    InspectMnemonic {
        mnemonic: String,
    },
    GetNetworkInfo,
    IsIronwoodActiveForWallet {
        wallet_id: WalletId,
    },
    FormatAmount {
        #[serde(with = "crate::models::amount_json::u64")]
        arrrtoshis: u64,
    },
    ParseAmount {
        arrr: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonEnvelope {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WalletService;

impl WalletService {
    pub const fn new() -> Self {
        Self
    }

    pub async fn execute(&self, request: WalletServiceRequest) -> Result<Value> {
        use crate as ffi;

        match request {
            WalletServiceRequest::GetBuildInfo => serialize(ffi::get_build_info()?),
            WalletServiceRequest::WalletRegistryExists => serialize(ffi::wallet_registry_exists()?),
            WalletServiceRequest::ListWallets => serialize(ffi::list_wallets()?),
            WalletServiceRequest::GetActiveWallet => serialize(ffi::get_active_wallet()?),
            WalletServiceRequest::CreateWallet {
                name,
                birthday_opt,
                mnemonic_language,
            } => serialize(ffi::create_wallet(
                name,
                None,
                birthday_opt,
                mnemonic_language,
            )?),
            WalletServiceRequest::RestoreWallet {
                name,
                mnemonic,
                birthday_opt,
                mnemonic_language,
            } => serialize(ffi::restore_wallet(
                name,
                mnemonic,
                birthday_opt,
                mnemonic_language,
            )?),
            WalletServiceRequest::ImportViewingWallet {
                name,
                sapling_viewing_key,
                ironwood_viewing_key,
                birthday,
            } => serialize(ffi::import_viewing_wallet(
                name,
                sapling_viewing_key,
                ironwood_viewing_key,
                birthday,
            )?),
            WalletServiceRequest::SwitchWallet { wallet_id } => {
                ffi::switch_wallet(wallet_id)?;
                Ok(ack())
            }
            WalletServiceRequest::RenameWallet {
                wallet_id,
                new_name,
            } => {
                ffi::rename_wallet(wallet_id, new_name)?;
                Ok(ack())
            }
            WalletServiceRequest::SetWalletBirthdayHeight {
                wallet_id,
                birthday_height,
            } => {
                ffi::set_wallet_birthday_height(wallet_id, birthday_height)?;
                Ok(ack())
            }
            WalletServiceRequest::DeleteWallet { wallet_id } => {
                ffi::delete_wallet(wallet_id)?;
                Ok(ack())
            }
            WalletServiceRequest::ConfigureWalletStorage {
                base_dir,
                passphrase,
            } => {
                ffi::configure_wallet_storage(base_dir, passphrase)?;
                Ok(ack())
            }
            WalletServiceRequest::SetAppPassphrase { passphrase } => {
                ffi::set_app_passphrase(passphrase)?;
                Ok(ack())
            }
            WalletServiceRequest::HasAppPassphrase => serialize(ffi::has_app_passphrase()?),
            WalletServiceRequest::VerifyAppPassphrase { passphrase } => {
                serialize(ffi::verify_app_passphrase(passphrase)?)
            }
            WalletServiceRequest::UnlockApp { passphrase } => {
                ffi::unlock_app(passphrase)?;
                Ok(ack())
            }
            WalletServiceRequest::ChangeAppPassphrase {
                current_passphrase,
                new_passphrase,
            } => {
                ffi::change_app_passphrase(current_passphrase, new_passphrase)?;
                Ok(ack())
            }
            WalletServiceRequest::ChangeAppPassphraseWithCached { new_passphrase } => {
                ffi::change_app_passphrase_with_cached(new_passphrase)?;
                Ok(ack())
            }
            WalletServiceRequest::CurrentReceiveAddress { wallet_id } => {
                serialize(ffi::current_receive_address(wallet_id)?)
            }
            WalletServiceRequest::NextReceiveAddress { wallet_id } => {
                serialize(ffi::next_receive_address(wallet_id)?)
            }
            WalletServiceRequest::LabelAddress {
                wallet_id,
                address,
                label,
            } => {
                ffi::label_address(wallet_id, address, label)?;
                Ok(ack())
            }
            WalletServiceRequest::SetAddressColorTag {
                wallet_id,
                address,
                color_tag,
            } => {
                ffi::set_address_color_tag(wallet_id, address, color_tag)?;
                Ok(ack())
            }
            WalletServiceRequest::ListAddresses { wallet_id } => {
                serialize(ffi::list_addresses(wallet_id)?)
            }
            WalletServiceRequest::ListAddressBalances { wallet_id, key_id } => {
                serialize(ffi::list_address_balances(wallet_id, key_id)?)
            }
            WalletServiceRequest::GetBalance { wallet_id } => {
                serialize(ffi::get_balance(wallet_id)?)
            }
            WalletServiceRequest::GetShieldedPoolBalances { wallet_id } => {
                serialize(ffi::get_shielded_pool_balances(wallet_id)?)
            }
            WalletServiceRequest::GetFeeInfo => serialize(ffi::get_fee_info()?),
            WalletServiceRequest::GetAutoConsolidationThreshold => {
                serialize(ffi::get_auto_consolidation_threshold()?)
            }
            WalletServiceRequest::GetAutoConsolidationCandidateCount { wallet_id } => {
                serialize(ffi::get_auto_consolidation_candidate_count(wallet_id)?)
            }
            WalletServiceRequest::GetSpendabilityStatus { wallet_id } => {
                serialize(ffi::get_spendability_status(wallet_id)?)
            }
            WalletServiceRequest::EnableWalletSigningProtection {
                wallet_id,
                session_credential,
            } => serialize(ffi::enable_wallet_signing_protection(
                wallet_id,
                session_credential,
            )?),
            WalletServiceRequest::UnlockWalletSigning {
                wallet_id,
                session_credential,
            } => serialize(ffi::unlock_wallet_signing(wallet_id, session_credential)?),
            WalletServiceRequest::LockWalletSigning { wallet_id } => {
                serialize(ffi::lock_wallet_signing(wallet_id)?)
            }
            WalletServiceRequest::LockAllWalletSigning => {
                ffi::lock_all_wallet_signing()?;
                Ok(ack())
            }
            WalletServiceRequest::GetWalletSigningStatus { wallet_id } => {
                serialize(ffi::get_wallet_signing_status(wallet_id)?)
            }
            WalletServiceRequest::ListKeyGroups { wallet_id } => {
                serialize(ffi::list_key_groups(wallet_id)?)
            }
            WalletServiceRequest::ExportKeyGroupKeys { wallet_id, key_id } => {
                serialize(ffi::export_key_group_keys(wallet_id, key_id)?)
            }
            WalletServiceRequest::ImportSpendingKey {
                wallet_id,
                sapling_key,
                ironwood_key,
                label,
                birthday_height,
            } => serialize(ffi::import_spending_key(
                wallet_id,
                sapling_key,
                ironwood_key,
                label,
                birthday_height,
            )?),
            WalletServiceRequest::ImportSpendingKeyVerified {
                wallet_id,
                pool,
                spending_key,
                expected_address,
                address_index,
                label,
                birthday_height,
            } => serialize(
                ffi::import_spending_key_verified(
                    wallet_id,
                    pool,
                    spending_key,
                    expected_address,
                    address_index,
                    label,
                    birthday_height,
                )
                .await?,
            ),
            WalletServiceRequest::ExportSeedRaw {
                wallet_id,
                mnemonic_language,
            } => serialize(ffi::export_seed_raw(wallet_id, mnemonic_language)?),
            WalletServiceRequest::ListTransactions { wallet_id, limit } => {
                serialize(ffi::list_transactions(wallet_id, limit)?)
            }
            WalletServiceRequest::ListIncomingDeposits { wallet_id, limit } => {
                serialize(ffi::list_incoming_deposits(wallet_id, limit)?)
            }
            WalletServiceRequest::ListTransactionsPage {
                wallet_id,
                cursor,
                page_size,
            } => serialize(ffi::list_transactions_page(wallet_id, cursor, page_size)?),
            WalletServiceRequest::QortalSyncStatus { wallet_id } => {
                serialize(ffi::qortal_sync_status(wallet_id)?)
            }
            WalletServiceRequest::QortalBalance { wallet_id } => {
                serialize(ffi::qortal_balance(wallet_id)?)
            }
            WalletServiceRequest::QortalListTransactions { wallet_id, limit } => {
                serialize(ffi::qortal_list_transactions(wallet_id, limit).await?)
            }
            WalletServiceRequest::QortalSend { wallet_id, request } => {
                serialize(ffi::qortal_send(wallet_id, request).await?)
            }
            WalletServiceRequest::QortalSendP2sh { wallet_id, request } => {
                serialize(ffi::qortal_send_p2sh(wallet_id, request).await?)
            }
            WalletServiceRequest::QortalRedeemP2sh { wallet_id, request } => {
                serialize(ffi::qortal_redeem_p2sh(wallet_id, request).await?)
            }
            WalletServiceRequest::ListNotes {
                wallet_id,
                all_notes,
            } => serialize(ffi::list_notes(wallet_id, all_notes)?),
            WalletServiceRequest::ClearWalletState { wallet_id } => {
                ffi::clear_wallet_state(wallet_id)?;
                Ok(ack())
            }
            WalletServiceRequest::FetchTransactionMemo {
                wallet_id,
                txid,
                output_index,
            } => serialize(ffi::fetch_transaction_memo(wallet_id, txid, output_index).await?),
            WalletServiceRequest::GetTransactionDetails { wallet_id, txid } => {
                serialize(ffi::get_transaction_details(wallet_id, txid).await?)
            }
            WalletServiceRequest::ExportPaymentDisclosures { wallet_id, txid } => {
                serialize(ffi::export_payment_disclosures(wallet_id, txid).await?)
            }
            WalletServiceRequest::ExportSaplingPaymentDisclosure {
                wallet_id,
                txid,
                output_index,
            } => serialize(
                ffi::export_sapling_payment_disclosure(wallet_id, txid, output_index).await?,
            ),
            WalletServiceRequest::ExportIronwoodPaymentDisclosure {
                wallet_id,
                txid,
                action_index,
            } => serialize(
                ffi::export_ironwood_payment_disclosure(wallet_id, txid, action_index).await?,
            ),
            WalletServiceRequest::VerifyPaymentDisclosure {
                wallet_id,
                disclosure,
            } => serialize(ffi::verify_payment_disclosure(wallet_id, disclosure).await?),
            WalletServiceRequest::ListAddressBook { wallet_id } => {
                serialize(ffi::list_address_book(wallet_id)?)
            }
            WalletServiceRequest::AddAddressBookEntry {
                wallet_id,
                address,
                label,
                notes,
                color_tag,
            } => serialize(ffi::add_address_book_entry(
                wallet_id, address, label, notes, color_tag,
            )?),
            WalletServiceRequest::UpdateAddressBookEntry {
                wallet_id,
                id,
                label,
                notes,
                color_tag,
                is_favorite,
            } => serialize(ffi::update_address_book_entry(
                wallet_id,
                id,
                label,
                notes,
                color_tag,
                is_favorite,
            )?),
            WalletServiceRequest::DeleteAddressBookEntry { wallet_id, id } => {
                ffi::delete_address_book_entry(wallet_id, id)?;
                Ok(ack())
            }
            WalletServiceRequest::ToggleAddressBookFavorite { wallet_id, id } => {
                serialize(ffi::toggle_address_book_favorite(wallet_id, id)?)
            }
            WalletServiceRequest::MarkAddressUsed { wallet_id, address } => {
                ffi::mark_address_used(wallet_id, address)?;
                Ok(ack())
            }
            WalletServiceRequest::GetLabelForAddress { wallet_id, address } => {
                serialize(ffi::get_label_for_address(wallet_id, address)?)
            }
            WalletServiceRequest::AddressExistsInBook { wallet_id, address } => {
                serialize(ffi::address_exists_in_book(wallet_id, address)?)
            }
            WalletServiceRequest::GetAddressBookCount { wallet_id } => {
                serialize(ffi::get_address_book_count(wallet_id)?)
            }
            WalletServiceRequest::GetAddressBookEntry { wallet_id, id } => {
                serialize(ffi::get_address_book_entry(wallet_id, id)?)
            }
            WalletServiceRequest::GetAddressBookEntryByAddress { wallet_id, address } => {
                serialize(ffi::get_address_book_entry_by_address(wallet_id, address)?)
            }
            WalletServiceRequest::SearchAddressBook { wallet_id, query } => {
                serialize(ffi::search_address_book(wallet_id, query)?)
            }
            WalletServiceRequest::GetAddressBookFavorites { wallet_id } => {
                serialize(ffi::get_address_book_favorites(wallet_id)?)
            }
            WalletServiceRequest::GetRecentlyUsedAddresses { wallet_id, limit } => {
                serialize(ffi::get_recently_used_addresses(wallet_id, limit)?)
            }
            WalletServiceRequest::IsValidShieldedAddress { address } => {
                serialize(ffi::is_valid_shielded_addr(address)?)
            }
            WalletServiceRequest::ValidateAddress { address } => {
                serialize(ffi::validate_address(address)?)
            }
            WalletServiceRequest::GetLightdEndpoint { wallet_id } => {
                serialize(ffi::get_lightd_endpoint(wallet_id)?)
            }
            WalletServiceRequest::GetLightdEndpointConfig { wallet_id } => {
                serialize(ffi::get_lightd_endpoint_config(wallet_id)?)
            }
            WalletServiceRequest::GetLightdEndpointPoolDiagnostics { wallet_id } => {
                serialize(ffi::get_lightd_endpoint_pool_diagnostics(wallet_id).await?)
            }
            WalletServiceRequest::SetLightdEndpoint {
                wallet_id,
                url,
                tls_pin_opt,
            } => {
                ffi::set_lightd_endpoint(wallet_id, url, tls_pin_opt)?;
                Ok(ack())
            }
            WalletServiceRequest::SetLightdEndpointPool {
                wallet_id,
                url,
                tls_pin_opt,
                failover_endpoints,
            } => {
                ffi::set_lightd_endpoint_pool(wallet_id, url, tls_pin_opt, failover_endpoints)?;
                Ok(ack())
            }
            WalletServiceRequest::GetTunnel => serialize(ffi::get_tunnel()?),
            WalletServiceRequest::SetTunnel { mode } => {
                ffi::set_tunnel(mode)?;
                Ok(ack())
            }
            WalletServiceRequest::BootstrapTunnel { mode } => {
                ffi::bootstrap_tunnel(mode).await?;
                Ok(ack())
            }
            WalletServiceRequest::ShutdownTransport => {
                ffi::shutdown_transport().await?;
                Ok(ack())
            }
            WalletServiceRequest::SetTorBridgeSettings {
                use_bridges,
                fallback_to_bridges,
                transport,
                bridge_lines,
                transport_path,
            } => {
                ffi::set_tor_bridge_settings(
                    use_bridges,
                    fallback_to_bridges,
                    transport,
                    bridge_lines,
                    transport_path,
                )
                .await?;
                Ok(ack())
            }
            WalletServiceRequest::GetTorStatus => serialize(ffi::get_tor_status().await?),
            WalletServiceRequest::RotateTorExit => {
                ffi::rotate_tor_exit().await?;
                Ok(ack())
            }
            WalletServiceRequest::FetchExternalText {
                url,
                accept,
                user_agent,
            } => serialize(ffi::fetch_external_text(url, accept, user_agent).await?),
            WalletServiceRequest::FetchExternalBytes {
                url,
                accept,
                user_agent,
            } => serialize(ffi::fetch_external_bytes(url, accept, user_agent).await?),
            WalletServiceRequest::DownloadExternalToFile {
                url,
                destination_path,
                accept,
                user_agent,
            } => {
                ffi::download_external_to_file(url, destination_path, accept, user_agent).await?;
                Ok(ack())
            }
            WalletServiceRequest::TestNode { url, tls_pin } => {
                serialize(ffi::test_node(url, tls_pin).await?)
            }
            WalletServiceRequest::StartSync { wallet_id, mode } => {
                ffi::start_sync(wallet_id, mode).await?;
                Ok(ack())
            }
            WalletServiceRequest::SyncStatus { wallet_id } => {
                serialize(ffi::sync_status(wallet_id)?)
            }
            WalletServiceRequest::CancelSync { wallet_id } => {
                ffi::cancel_sync(wallet_id).await?;
                Ok(ack())
            }
            WalletServiceRequest::Rescan {
                wallet_id,
                from_height,
            } => {
                ffi::rescan(wallet_id, from_height).await?;
                Ok(ack())
            }
            WalletServiceRequest::BuildTx {
                wallet_id,
                outputs,
                fee_opt,
            } => serialize(ffi::build_tx(wallet_id, outputs, fee_opt)?),
            WalletServiceRequest::SignTx { wallet_id, pending } => {
                serialize(ffi::sign_tx(wallet_id, pending)?)
            }
            WalletServiceRequest::BroadcastTx { wallet_id, signed } => match wallet_id {
                Some(wallet_id) => {
                    serialize(ffi::broadcast_tx_for_wallet(wallet_id, signed).await?)
                }
                None => serialize(ffi::broadcast_tx(signed).await?),
            },
            WalletServiceRequest::StartSeedExport { wallet_id } => {
                serialize(ffi::start_seed_export(wallet_id)?)
            }
            WalletServiceRequest::AcknowledgeSeedWarning => {
                serialize(ffi::acknowledge_seed_warning()?)
            }
            WalletServiceRequest::CompleteSeedBiometric { success } => {
                serialize(ffi::complete_seed_biometric(success)?)
            }
            WalletServiceRequest::SkipSeedBiometric => serialize(ffi::skip_seed_biometric()?),
            WalletServiceRequest::ExportSeedWithPassphrase {
                wallet_id,
                passphrase,
                mnemonic_language,
            } => serialize(ffi::export_seed_with_passphrase(
                wallet_id,
                passphrase,
                mnemonic_language,
            )?),
            WalletServiceRequest::ExportSeedWithCachedPassphrase {
                wallet_id,
                mnemonic_language,
            } => serialize(ffi::export_seed_with_cached_passphrase(
                wallet_id,
                mnemonic_language,
            )?),
            WalletServiceRequest::CancelSeedExport => {
                ffi::cancel_seed_export()?;
                Ok(ack())
            }
            WalletServiceRequest::GetSeedExportState => serialize(ffi::get_seed_export_state()?),
            WalletServiceRequest::GetSeedExportWarnings => {
                serialize(ffi::get_seed_export_warnings()?)
            }
            WalletServiceRequest::ExportSaplingViewingKey { wallet_id } => {
                serialize(ffi::export_sapling_viewing_key(wallet_id)?)
            }
            WalletServiceRequest::ExportIronwoodViewingKey { wallet_id } => {
                serialize(ffi::export_ironwood_viewing_key(wallet_id)?)
            }
            WalletServiceRequest::ExportSaplingViewingKeySecure { wallet_id } => {
                serialize(ffi::export_sapling_viewing_key_secure(wallet_id)?)
            }
            WalletServiceRequest::ImportSaplingViewingKeyAsWatchOnly {
                name,
                sapling_viewing_key,
                birthday_height,
            } => serialize(ffi::import_sapling_viewing_key_as_watch_only(
                name,
                sapling_viewing_key,
                birthday_height,
            )?),
            WalletServiceRequest::GetWatchOnlyCapabilities { wallet_id } => {
                serialize(ffi::get_watch_only_capabilities(wallet_id)?)
            }
            WalletServiceRequest::GetWatchOnlyBanner { wallet_id } => {
                serialize(ffi::get_watch_only_banner(wallet_id)?)
            }
            WalletServiceRequest::GetIvkClipboardRemaining => {
                serialize(ffi::get_ivk_clipboard_remaining()?)
            }
            WalletServiceRequest::SetPanicPin { pin } => {
                ffi::set_panic_pin(pin)?;
                Ok(ack())
            }
            WalletServiceRequest::HasPanicPin => serialize(ffi::has_panic_pin()?),
            WalletServiceRequest::VerifyPanicPin { pin } => serialize(ffi::verify_panic_pin(pin)?),
            WalletServiceRequest::IsDecoyMode => serialize(ffi::is_decoy_mode()?),
            WalletServiceRequest::GetVaultMode => serialize(ffi::get_vault_mode()?),
            WalletServiceRequest::ClearPanicPin => {
                ffi::clear_panic_pin()?;
                Ok(ack())
            }
            WalletServiceRequest::SetDuressPassphrase { custom_passphrase } => {
                ffi::set_duress_passphrase(custom_passphrase)?;
                Ok(ack())
            }
            WalletServiceRequest::HasDuressPassphrase => serialize(ffi::has_duress_passphrase()?),
            WalletServiceRequest::ClearDuressPassphrase => {
                ffi::clear_duress_passphrase()?;
                Ok(ack())
            }
            WalletServiceRequest::VerifyDuressPassphrase { passphrase } => {
                serialize(ffi::verify_duress_passphrase(passphrase)?)
            }
            WalletServiceRequest::SetDecoyWalletName { name } => {
                ffi::set_decoy_wallet_name(name)?;
                Ok(ack())
            }
            WalletServiceRequest::ExitDecoyMode { passphrase } => {
                ffi::exit_decoy_mode(passphrase)?;
                Ok(ack())
            }
            WalletServiceRequest::SetDebugLoggingEnabled { enabled } => {
                ffi::set_debug_logging_enabled(enabled)?;
                Ok(ack())
            }
            WalletServiceRequest::GetDebugLoggingEnabled => {
                serialize(ffi::get_debug_logging_enabled()?)
            }
            WalletServiceRequest::ClearDebugLogs => {
                ffi::clear_debug_logs()?;
                Ok(ack())
            }
            WalletServiceRequest::GetSyncLogs { wallet_id, limit } => {
                serialize(ffi::get_sync_logs(wallet_id, limit)?)
            }
            WalletServiceRequest::GetCheckpointDetails { wallet_id, height } => {
                serialize(ffi::get_checkpoint_details(wallet_id, height)?)
            }
            WalletServiceRequest::ValidateConsensusBranch { wallet_id } => {
                serialize(ffi::validate_consensus_branch(wallet_id).await?)
            }
            WalletServiceRequest::GenerateMnemonic {
                word_count,
                mnemonic_language,
            } => serialize(ffi::generate_mnemonic(word_count, mnemonic_language)?),
            WalletServiceRequest::ValidateMnemonic {
                mnemonic,
                mnemonic_language,
            } => serialize(ffi::validate_mnemonic(mnemonic, mnemonic_language)?),
            WalletServiceRequest::InspectMnemonic { mnemonic } => {
                serialize(ffi::inspect_mnemonic(mnemonic)?)
            }
            WalletServiceRequest::GetNetworkInfo => serialize(ffi::get_network_info()?),
            WalletServiceRequest::IsIronwoodActiveForWallet { wallet_id } => {
                serialize(ffi::is_ironwood_active_for_wallet(wallet_id)?)
            }
            WalletServiceRequest::FormatAmount { arrrtoshis } => {
                serialize(ffi::format_amount(arrrtoshis)?)
            }
            WalletServiceRequest::ParseAmount { arrr } => {
                serialize_amount(ffi::parse_amount(arrr)?)
            }
        }
    }

    pub fn execute_blocking(&self, request: WalletServiceRequest) -> Result<Value> {
        // Use a process-wide persistent runtime so background tasks spawned by a
        // request keep running after the call returns. `start_sync` spawns the
        // sync engine onto this runtime; with the previous per-call
        // current-thread runtime, that runtime was dropped the moment
        // `block_on` returned, which immediately aborted the sync task. A host
        // that drives the service entirely through `execute_blocking` (e.g. the
        // React Native binding) could therefore never make sync progress.
        crate::runtime::block_on(self.execute(request))
    }

    pub fn execute_json(&self, request_json: &str, pretty: bool) -> String {
        let response = match serde_json::from_str::<WalletServiceRequest>(request_json) {
            Ok(request) => match self.execute_blocking(request) {
                Ok(result) => JsonEnvelope {
                    ok: true,
                    result: Some(result),
                    error: None,
                },
                Err(err) => JsonEnvelope {
                    ok: false,
                    result: None,
                    error: Some(err.to_string()),
                },
            },
            Err(err) => JsonEnvelope {
                ok: false,
                result: None,
                error: Some(format!("Invalid request JSON: {}", err)),
            },
        };

        if pretty {
            serde_json::to_string_pretty(&response).unwrap_or_else(|_| {
                "{\"ok\":false,\"error\":\"Failed to serialize response\"}".to_string()
            })
        } else {
            serde_json::to_string(&response).unwrap_or_else(|_| {
                "{\"ok\":false,\"error\":\"Failed to serialize response\"}".to_string()
            })
        }
    }
}

fn ack() -> Value {
    json!({ "acknowledged": true })
}

fn serialize<T: Serialize>(value: T) -> Result<Value> {
    Ok(to_value(value)?)
}

fn serialize_amount(value: u64) -> Result<Value> {
    Ok(Value::String(value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incoming_deposit_request_uses_the_public_json_method() {
        let request = serde_json::from_value::<WalletServiceRequest>(json!({
            "method": "list_incoming_deposits",
            "wallet_id": "merchant-wallet",
            "limit": 25
        }))
        .unwrap();

        assert!(matches!(
            request,
            WalletServiceRequest::ListIncomingDeposits {
                wallet_id,
                limit: Some(25)
            } if wallet_id == "merchant-wallet"
        ));
    }

    #[test]
    fn verified_spending_key_import_request_uses_explicit_non_heuristic_fields() {
        let request = serde_json::from_value::<WalletServiceRequest>(json!({
            "method": "import_spending_key_verified",
            "wallet_id": "recovery-wallet",
            "pool": "sapling",
            "spending_key": "secret-extended-key-main1redacted",
            "expected_address": "zs1expected",
            "address_index": 7,
            "label": "Recovered wallet",
            "birthday_height": 123
        }))
        .unwrap();

        assert!(matches!(
            request,
            WalletServiceRequest::ImportSpendingKeyVerified {
                wallet_id,
                pool: VerifiedSpendingKeyPool::Sapling,
                spending_key,
                expected_address,
                address_index: 7,
                label: Some(label),
                birthday_height: 123,
            } if wallet_id == "recovery-wallet"
                && spending_key == "secret-extended-key-main1redacted"
                && expected_address == "zs1expected"
                && label == "Recovered wallet"
        ));
    }

    #[test]
    fn verified_spending_key_import_result_contains_no_key_material() {
        let value = serialize(VerifiedSpendingKeyImport {
            key_id: 42,
            pool: VerifiedSpendingKeyPool::Ironwood,
            address: "pirate1verified".to_string(),
            address_index: 3,
            birthday_height: 100,
            already_imported: false,
            rescan_required: true,
            required_rescan_from_height: Some(90),
        })
        .unwrap();

        assert_eq!(value["key_id"], 42);
        assert_eq!(value["pool"], "ironwood");
        assert_eq!(value["address"], "pirate1verified");
        assert_eq!(value["required_rescan_from_height"], 90);
        assert!(value.get("spending_key").is_none());
        assert!(value.get("sapling_key").is_none());
        assert!(value.get("ironwood_key").is_none());
    }

    #[test]
    fn incoming_deposit_json_preserves_stable_identity_and_amount_precision() {
        let value = serialize(AddressedDeposit {
            txid: "ab".repeat(32),
            pool: crate::ShieldedAddressType::Ironwood,
            output_index: 4,
            address: "pirate1merchant".to_string(),
            address_scope: DepositAddressScope::External,
            height: Some(123),
            timestamp: Some(456),
            value: 9_007_199_254_740_993,
            confirmations: 7,
            confirmed: true,
        })
        .unwrap();

        assert_eq!(value["pool"], "Ironwood");
        assert_eq!(value["output_index"], 4);
        assert_eq!(value["address_scope"], "external");
        assert_eq!(value["value"], "9007199254740993");
        assert_eq!(value["confirmations"], 7);
    }
}
