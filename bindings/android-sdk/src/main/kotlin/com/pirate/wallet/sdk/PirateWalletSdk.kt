package com.pirate.wallet.sdk

import org.json.JSONArray
import org.json.JSONObject

internal object NativeBridge {
    init {
        System.loadLibrary("pirate_ffi_native")
    }

    external fun invokeJson(requestJson: String, pretty: Boolean = false): String
}

public interface PirateWalletNativeInvoker {
    fun invoke(requestJson: String, pretty: Boolean = false): String
}

public class PirateWalletCInvoker : PirateWalletNativeInvoker {
    override fun invoke(requestJson: String, pretty: Boolean): String =
        NativeBridge.invokeJson(requestJson, pretty)
}

public open class PirateWalletSdkException(
    message: String,
    cause: Throwable? = null,
) : RuntimeException(message, cause)

public data class BuildInfo(
    val version: String,
    val gitCommit: String,
    val buildDate: String,
    val rustVersion: String,
    val targetTriple: String,
)

public data class WalletMeta(
    val id: String,
    val name: String,
    val createdAt: Long,
    val watchOnly: Boolean,
    val birthdayHeight: Int,
    val networkType: NetworkType?,
)

public data class CreateWalletRequest(
    val name: String,
    val birthdayHeight: Int? = null,
)

public data class RestoreWalletRequest(
    val name: String,
    val mnemonic: String,
    val passphrase: String? = null,
    val birthdayHeight: Int? = null,
)

public data class ImportViewingWalletRequest(
    val name: String,
    val saplingViewingKey: String? = null,
    val orchardViewingKey: String? = null,
    val birthdayHeight: Int,
)

public data class TransactionOutput(
    val address: String,
    val amount: Long,
    val memo: String? = null,
)

public data class BuildTransactionRequest(
    val walletId: String,
    val outputs: List<TransactionOutput>,
    val fee: Long? = null,
)

public data class SyncRequest(
    val walletId: String,
    val mode: SyncMode = SyncMode.Compact,
)

public data class RescanRequest(
    val walletId: String,
    val fromHeight: Int,
)

public data class Balance(
    val total: Long,
    val spendable: Long,
    val pending: Long,
)

public data class ShieldedPoolBalances(
    val sapling: Balance,
    val orchard: Balance,
)

public data class TransactionInfo(
    val txId: String,
    val height: Int?,
    val timestamp: Long,
    val amount: Long,
    val fee: Long,
    val memo: String?,
    val confirmed: Boolean,
)

public data class PendingTransaction(
    val id: String,
    val outputs: List<TransactionOutput>,
    val totalAmount: Long,
    val fee: Long,
    val change: Long,
    val inputTotal: Long,
    val numInputs: Int,
    val expiryHeight: Int,
    val createdAt: Long,
) {
    public val totalSendValue: Long
        get() = totalAmount + fee

    public val hasMemo: Boolean
        get() = outputs.any { it.memo != null }
}

public data class SignedTransaction(
    val txId: String,
    val raw: ByteArray,
    val size: Int,
) {
    public fun rawHex(): String = raw.joinToString(separator = "") { byte ->
        ((byte.toInt() and 0xff) + 0x100).toString(16).takeLast(2)
    }
}

public data class FeeInfo(
    val defaultFee: Long,
    val minFee: Long,
    val maxFee: Long,
    val feePerOutput: Long,
    val memoFeeMultiplier: Double,
)

public enum class NetworkType(val jsonValue: String) {
    Mainnet("mainnet"),
    Testnet("testnet"),
    Regtest("regtest");

    public companion object {
        fun fromJson(value: Any?): NetworkType? = when (value?.toString()?.lowercase()) {
            "mainnet" -> Mainnet
            "testnet" -> Testnet
            "regtest" -> Regtest
            else -> null
        }
    }
}

public enum class SyncMode {
    Compact,
    Deep,
}

public enum class SyncStage {
    Headers,
    Notes,
    Witness,
    Verify,
}

public data class SyncStatus(
    val localHeight: Long,
    val targetHeight: Long,
    val percent: Double,
    val eta: Long?,
    val stage: SyncStage,
    val lastCheckpoint: Long?,
    val blocksPerSecond: Double,
    val notesDecrypted: Long,
    val lastBatchMs: Long,
) {
    public fun isSyncing(): Boolean = localHeight < targetHeight && targetHeight > 0

    public fun isComplete(): Boolean = localHeight >= targetHeight && targetHeight > 0

    public fun etaFormatted(): String = when {
        eta == null -> "Calculating..."
        eta > 3600 -> "${eta / 3600}h ${(eta % 3600) / 60}m"
        eta > 60 -> "${eta / 60}m ${eta % 60}s"
        else -> "${eta}s"
    }

    public fun stageName(): String = when (stage) {
        SyncStage.Headers -> "Fetching Headers"
        SyncStage.Notes -> "Scanning Notes"
        SyncStage.Witness -> "Building Witnesses"
        SyncStage.Verify -> "Synching Chain"
    }
}

public data class CheckpointInfo(
    val height: Int,
    val timestamp: Long,
)

public class PirateWalletSdk(
    private val invoker: PirateWalletNativeInvoker = PirateWalletCInvoker(),
) {
    public val advancedKeyManagement: PirateWalletAdvancedKeyManagement =
        PirateWalletAdvancedKeyManagement(this)

    public fun invoke(requestJson: String, pretty: Boolean = false): String =
        invoker.invoke(requestJson, pretty)

    public fun createSynchronizer(
        walletId: String,
        config: PirateWalletSynchronizer.Config = PirateWalletSynchronizer.Config(),
    ): PirateWalletSynchronizer = PirateWalletSynchronizer(
        sdk = this,
        walletId = walletId,
        config = config,
    )

    public fun buildInfoJson(pretty: Boolean = false): String =
        invoke("""{"method":"get_build_info"}""", pretty)

    public fun buildInfo(): BuildInfo =
        parseBuildInfo(invokeResult("get_build_info"))

    public fun walletRegistryExists(): Boolean =
        parseBoolean(invokeResult("wallet_registry_exists"))

    public fun listWallets(): List<WalletMeta> =
        parseWalletMetaList(invokeResult("list_wallets"))

    public fun getActiveWalletId(): String? =
        parseStringOrNull(invokeResult("get_active_wallet"))

    public fun getActiveWallet(): WalletMeta? {
        val activeWalletId = getActiveWalletId() ?: return null
        return listWallets().firstOrNull { it.id == activeWalletId }
    }

    public fun getWallet(walletId: String): WalletMeta? =
        listWallets().firstOrNull { it.id == walletId }

    public fun createWallet(request: CreateWalletRequest): String =
        parseString(invokeResult("create_wallet", "name" to request.name, "birthday_opt" to request.birthdayHeight))

    public fun createWallet(name: String, birthdayHeight: Int? = null): String =
        createWallet(CreateWalletRequest(name = name, birthdayHeight = birthdayHeight))

    public fun restoreWallet(request: RestoreWalletRequest): String =
        parseString(
            invokeResult(
                "restore_wallet",
                "name" to request.name,
                "mnemonic" to request.mnemonic,
                "passphrase_opt" to request.passphrase,
                "birthday_opt" to request.birthdayHeight,
            ),
        )

    public fun restoreWallet(
        name: String,
        mnemonic: String,
        passphrase: String? = null,
        birthdayHeight: Int? = null,
    ): String = restoreWallet(
        RestoreWalletRequest(
            name = name,
            mnemonic = mnemonic,
            passphrase = passphrase,
            birthdayHeight = birthdayHeight,
        ),
    )

    public fun importViewingWallet(request: ImportViewingWalletRequest): String =
        parseString(
            invokeResult(
                "import_viewing_wallet",
                "name" to request.name,
                "sapling_viewing_key" to request.saplingViewingKey,
                "orchard_viewing_key" to request.orchardViewingKey,
                "birthday" to request.birthdayHeight,
            ),
        )

    public fun importViewingWallet(
        name: String,
        saplingViewingKey: String? = null,
        orchardViewingKey: String? = null,
        birthdayHeight: Int,
    ): String = importViewingWallet(
        ImportViewingWalletRequest(
            name = name,
            saplingViewingKey = saplingViewingKey,
            orchardViewingKey = orchardViewingKey,
            birthdayHeight = birthdayHeight,
        ),
    )

    public fun switchWallet(walletId: String) {
        invokeUnit("switch_wallet", "wallet_id" to walletId)
    }

    public fun renameWallet(walletId: String, newName: String) {
        invokeUnit("rename_wallet", "wallet_id" to walletId, "new_name" to newName)
    }

    public fun deleteWallet(walletId: String) {
        invokeUnit("delete_wallet", "wallet_id" to walletId)
    }

    public fun setWalletBirthdayHeight(walletId: String, birthdayHeight: Int) {
        invokeUnit("set_wallet_birthday_height", "wallet_id" to walletId, "birthday_height" to birthdayHeight)
    }

    public fun getLatestBirthdayHeight(walletId: String): Int? =
        getWallet(walletId)?.birthdayHeight

    public fun generateMnemonic(wordCount: Int? = null): String =
        parseString(invokeResult("generate_mnemonic", "word_count" to wordCount))

    public fun validateMnemonic(mnemonic: String): Boolean =
        parseBoolean(invokeResult("validate_mnemonic", "mnemonic" to mnemonic))

    public fun getNetworkInfo(): NetworkInfo =
        parseNetworkInfo(invokeResult("get_network_info"))

    public fun isValidShieldedAddr(address: String): Boolean =
        parseBoolean(invokeResult("is_valid_shielded_address", "address" to address))

    public fun validateAddress(address: String): AddressValidation =
        parseAddressValidation(invokeResult("validate_address", "address" to address))

    public fun validateConsensusBranch(walletId: String): ConsensusBranchValidation =
        parseConsensusBranchValidation(
            invokeResult("validate_consensus_branch", "wallet_id" to walletId),
        )

    public fun formatAmount(arrrtoshis: Long): String =
        parseString(invokeResult("format_amount", "arrrtoshis" to arrrtoshis))

    public fun parseAmount(arrr: String): Long =
        parseLongValue(invokeResult("parse_amount", "arrr" to arrr))

    public fun getCurrentReceiveAddress(walletId: String): String =
        getCurrentAddress(walletId)

    public fun getCurrentAddress(walletId: String): String =
        parseString(invokeResult("current_receive_address", "wallet_id" to walletId))

    public fun getNextReceiveAddress(walletId: String): String =
        getNextAddress(walletId)

    public fun getNextAddress(walletId: String): String =
        parseString(invokeResult("next_receive_address", "wallet_id" to walletId))

    public fun listAddresses(walletId: String): List<AddressInfo> =
        parseAddressInfoList(invokeResult("list_addresses", "wallet_id" to walletId))

    public fun listAddressBalances(walletId: String, keyId: Long? = null): List<AddressBalanceInfo> =
        parseAddressBalanceInfoList(
            invokeResult(
                "list_address_balances",
                "wallet_id" to walletId,
                "key_id" to keyId,
            ),
        )

    public fun getBalance(walletId: String): Balance =
        parseBalance(invokeResult("get_balance", "wallet_id" to walletId))

    public fun getShieldedPoolBalances(walletId: String): ShieldedPoolBalances =
        parseShieldedPoolBalances(
            invokeResult("get_shielded_pool_balances", "wallet_id" to walletId),
        )

    public fun getSpendabilityStatus(walletId: String): SpendabilityStatus =
        parseSpendabilityStatus(invokeResult("get_spendability_status", "wallet_id" to walletId))

    public fun listTransactions(walletId: String, limit: Int? = null): List<TransactionInfo> =
        parseTransactionInfoList(invokeResult("list_transactions", "wallet_id" to walletId, "limit" to limit))

    public fun fetchTransactionMemo(walletId: String, txId: String, outputIndex: Int? = null): String? =
        parseStringOrNull(
            invokeResult(
                "fetch_transaction_memo",
                "wallet_id" to walletId,
                "txid" to txId,
                "output_index" to outputIndex,
            ),
        )

    public fun getTransactionDetails(walletId: String, txId: String): TransactionDetails? =
        parseTransactionDetails(
            invokeResult(
                "get_transaction_details",
                "wallet_id" to walletId,
                "txid" to txId,
            ),
        )

    public fun getFeeInfo(): FeeInfo =
        parseFeeInfo(invokeResult("get_fee_info"))

    public fun startSync(request: SyncRequest) {
        invokeUnit("start_sync", "wallet_id" to request.walletId, "mode" to request.mode)
    }

    public fun startSync(walletId: String, mode: SyncMode = SyncMode.Compact) {
        startSync(SyncRequest(walletId = walletId, mode = mode))
    }

    public fun getSyncStatus(walletId: String): SyncStatus =
        parseSyncStatus(invokeResult("sync_status", "wallet_id" to walletId))

    public fun cancelSync(walletId: String) {
        invokeUnit("cancel_sync", "wallet_id" to walletId)
    }

    public fun rescan(request: RescanRequest) {
        invokeUnit("rescan", "wallet_id" to request.walletId, "from_height" to request.fromHeight)
    }

    public fun rescan(walletId: String, fromHeight: Int) {
        rescan(RescanRequest(walletId = walletId, fromHeight = fromHeight))
    }

    public fun buildTransaction(request: BuildTransactionRequest): PendingTransaction =
        parsePendingTransaction(
            invokeResult(
                "build_tx",
                "wallet_id" to request.walletId,
                "outputs" to request.outputs,
                "fee_opt" to request.fee,
            ),
        )

    public fun buildTransaction(
        walletId: String,
        outputs: List<TransactionOutput>,
        fee: Long? = null,
    ): PendingTransaction = buildTransaction(
        BuildTransactionRequest(walletId = walletId, outputs = outputs, fee = fee),
    )

    public fun buildTransaction(
        walletId: String,
        output: TransactionOutput,
        fee: Long? = null,
    ): PendingTransaction = buildTransaction(walletId, listOf(output), fee)

    public fun signTransaction(walletId: String, pending: PendingTransaction): SignedTransaction =
        parseSignedTransaction(
            invokeResult(
                "sign_tx",
                "wallet_id" to walletId,
                "pending" to pending,
            ),
        )

    public fun broadcastTransaction(signed: SignedTransaction): String =
        parseString(invokeResult("broadcast_tx", "signed" to signed))

    public fun send(
        walletId: String,
        outputs: List<TransactionOutput>,
        fee: Long? = null,
    ): String {
        val signed = signTransaction(walletId, buildTransaction(walletId, outputs, fee))
        return broadcastTransaction(signed)
    }

    public fun send(
        walletId: String,
        output: TransactionOutput,
        fee: Long? = null,
    ): String = send(walletId, listOf(output), fee)

    public fun exportSaplingViewingKey(walletId: String): String =
        parseString(
            invokeResult("export_sapling_viewing_key", "wallet_id" to walletId),
        )

    public fun exportOrchardViewingKey(walletId: String): String =
        parseString(invokeResult("export_orchard_viewing_key", "wallet_id" to walletId))

    public fun importSaplingViewingKeyAsWatchOnly(
        request: ImportWatchOnlyWalletRequest,
    ): String =
        parseString(
            invokeResult(
                "import_sapling_viewing_key_as_watch_only",
                "name" to request.name,
                "sapling_viewing_key" to request.saplingViewingKey,
                "birthday_height" to request.birthdayHeight,
            ),
        )

    public fun importSaplingViewingKeyAsWatchOnly(
        name: String,
        saplingViewingKey: String,
        birthdayHeight: Int,
    ): String = importSaplingViewingKeyAsWatchOnly(
            ImportWatchOnlyWalletRequest(
                name = name,
                saplingViewingKey = saplingViewingKey,
                birthdayHeight = birthdayHeight,
            ),
        )

    public fun getWatchOnlyCapabilities(walletId: String): WatchOnlyCapabilities =
        parseWatchOnlyCapabilities(invokeResult("get_watch_only_capabilities", "wallet_id" to walletId))

    internal fun invokeResult(method: String, vararg params: Pair<String, Any?>): Any? =
        extractResult(invokeEnvelope(method, *params))

    private fun invokeUnit(method: String, vararg params: Pair<String, Any?>) {
        invokeEnvelope(method, *params)
    }

    private fun invokeEnvelope(method: String, vararg params: Pair<String, Any?>): JSONObject {
        val requestJson = buildRequestJson(method, *params)
        val responseJson = invoke(requestJson)
        return parseEnvelope(responseJson)
    }
}

public class PirateWalletAdvancedKeyManagement internal constructor(
    private val sdk: PirateWalletSdk,
) {
    public fun listKeyGroups(walletId: String): List<KeyGroupInfo> =
        parseKeyGroupInfoList(
            sdk.invokeResult("list_key_groups", "wallet_id" to walletId),
        )

    public fun exportKeyGroupKeys(walletId: String, keyId: Long): KeyExportInfo =
        parseKeyExportInfo(
            sdk.invokeResult(
                "export_key_group_keys",
                "wallet_id" to walletId,
                "key_id" to keyId,
            ),
        )

    public fun importSpendingKey(request: ImportSpendingKeyRequest): Long =
        parseLongValue(
            sdk.invokeResult(
                "import_spending_key",
                "wallet_id" to request.walletId,
                "sapling_key" to request.saplingSpendingKey,
                "orchard_key" to request.orchardSpendingKey,
                "label" to request.label,
                "birthday_height" to request.birthdayHeight,
            ),
        )

    public fun importSpendingKey(
        walletId: String,
        birthdayHeight: Int,
        saplingSpendingKey: String? = null,
        orchardSpendingKey: String? = null,
        label: String? = null,
    ): Long = importSpendingKey(
        ImportSpendingKeyRequest(
            walletId = walletId,
            saplingSpendingKey = saplingSpendingKey,
            orchardSpendingKey = orchardSpendingKey,
            label = label,
            birthdayHeight = birthdayHeight,
        ),
    )

    public fun exportSeed(walletId: String): String =
        parseString(
            sdk.invokeResult(
                "export_seed_raw",
                "wallet_id" to walletId,
            ),
        )
}

private fun buildRequestJson(method: String, vararg params: Pair<String, Any?>): String {
    val request = JSONObject().put("method", method)
    for ((name, value) in params) {
        if (value == null) {
            continue
        }
        request.put(name, toJsonCompatible(value))
    }
    return request.toString()
}

private fun parseEnvelope(responseJson: String): JSONObject {
    val envelope = try {
        JSONObject(responseJson)
    } catch (e: Exception) {
        throw PirateWalletSdkException("Invalid JSON response from wallet service", e)
    }

    if (!envelope.optBoolean("ok", false)) {
        throw PirateWalletSdkException(
            envelope.optString("error", "Wallet service request failed"),
        )
    }

    return envelope
}

private fun extractResult(envelope: JSONObject): Any? =
    if (!envelope.has("result") || envelope.isNull("result")) null else envelope.get("result")

private fun parseBuildInfo(value: Any?): BuildInfo {
    val json = value.requireObject("build info")
    return BuildInfo(
        version = json.requireString("version"),
        gitCommit = json.requireString("git_commit"),
        buildDate = json.requireString("build_date"),
        rustVersion = json.requireString("rust_version"),
        targetTriple = json.requireString("target_triple"),
    )
}

private fun parseWalletMetaList(value: Any?): List<WalletMeta> {
    val array = value.requireArray("wallet list")
    return array.toList { parseWalletMeta(it) }
}

private fun parseWalletMeta(value: Any?): WalletMeta {
    val json = value.requireObject("wallet meta")
    return WalletMeta(
        id = json.requireString("id"),
        name = json.requireString("name"),
        createdAt = json.requireLong("created_at"),
        watchOnly = json.requireBoolean("watch_only"),
        birthdayHeight = json.requireInt("birthday_height"),
        networkType = NetworkType.fromJson(json.opt("network_type")),
    )
}

private fun parseBalance(value: Any?): Balance {
    val json = value.requireObject("balance")
    return Balance(
        total = json.requireLong("total"),
        spendable = json.requireLong("spendable"),
        pending = json.requireLong("pending"),
    )
}

private fun parseShieldedPoolBalances(value: Any?): ShieldedPoolBalances {
    val json = value.requireObject("shielded pool balances")
    return ShieldedPoolBalances(
        sapling = parseBalance(json.requireObject("sapling")),
        orchard = parseBalance(json.requireObject("orchard")),
    )
}

private fun parseAddressInfoList(value: Any?): List<AddressInfo> {
    val array = value.requireArray("address list")
    return array.toList(::parseAddressInfo)
}

private fun parseAddressInfo(value: Any?): AddressInfo {
    val json = value.requireObject("address info")
    return AddressInfo(
        address = json.requireString("address"),
        diversifierIndex = json.requireInt("diversifier_index"),
        label = json.nullableString("label"),
        createdAt = json.requireLong("created_at"),
        colorTag = AddressBookColorTag.fromJson(json.opt("color_tag")),
    )
}

private fun parseAddressBalanceInfoList(value: Any?): List<AddressBalanceInfo> {
    val array = value.requireArray("address balance list")
    return array.toList(::parseAddressBalanceInfo)
}

private fun parseAddressBalanceInfo(value: Any?): AddressBalanceInfo {
    val json = value.requireObject("address balance info")
    return AddressBalanceInfo(
        address = json.requireString("address"),
        balance = json.requireLong("balance"),
        spendable = json.requireLong("spendable"),
        pending = json.requireLong("pending"),
        keyId = json.nullableLong("key_id"),
        addressId = json.requireLong("address_id"),
        label = json.nullableString("label"),
        createdAt = json.requireLong("created_at"),
        colorTag = AddressBookColorTag.fromJson(json.opt("color_tag")),
        diversifierIndex = json.requireInt("diversifier_index"),
    )
}

private fun parseSpendabilityStatus(value: Any?): SpendabilityStatus {
    val json = value.requireObject("spendability status")
    return SpendabilityStatus(
        spendable = json.requireBoolean("spendable"),
        rescanRequired = json.requireBoolean("rescan_required"),
        targetHeight = json.requireLong("target_height"),
        anchorHeight = json.requireLong("anchor_height"),
        validatedAnchorHeight = json.requireLong("validated_anchor_height"),
        repairQueued = json.requireBoolean("repair_queued"),
        reasonCode = json.requireString("reason_code"),
    )
}

private fun parseTransactionInfoList(value: Any?): List<TransactionInfo> {
    val array = value.requireArray("transaction list")
    return array.toList { parseTransactionInfo(it) }
}

private fun parseTransactionInfo(value: Any?): TransactionInfo {
    val json = value.requireObject("transaction info")
    return TransactionInfo(
        txId = json.requireString("txid"),
        height = json.nullableInt("height"),
        timestamp = json.requireLong("timestamp"),
        amount = json.requireLong("amount"),
        fee = json.requireLong("fee"),
        memo = json.nullableString("memo"),
        confirmed = json.requireBoolean("confirmed"),
    )
}

private fun parsePendingTransaction(value: Any?): PendingTransaction {
    val json = value.requireObject("pending transaction")
    return PendingTransaction(
        id = json.requireString("id"),
        outputs = json.requireArray("outputs").toList(::parseTransactionOutput),
        totalAmount = json.requireLong("total_amount"),
        fee = json.requireLong("fee"),
        change = json.requireLong("change"),
        inputTotal = json.requireLong("input_total"),
        numInputs = json.requireInt("num_inputs"),
        expiryHeight = json.requireInt("expiry_height"),
        createdAt = json.requireLong("created_at"),
    )
}

private fun parseSignedTransaction(value: Any?): SignedTransaction {
    val json = value.requireObject("signed transaction")
    return SignedTransaction(
        txId = json.requireString("txid"),
        raw = json.requireArray("raw").toByteArray(),
        size = json.requireInt("size"),
    )
}

private fun parseFeeInfo(value: Any?): FeeInfo {
    val json = value.requireObject("fee info")
    return FeeInfo(
        defaultFee = json.requireLong("default_fee"),
        minFee = json.requireLong("min_fee"),
        maxFee = json.requireLong("max_fee"),
        feePerOutput = json.requireLong("fee_per_output"),
        memoFeeMultiplier = json.requireDouble("memo_fee_multiplier"),
    )
}

private fun parseSyncStatus(value: Any?): SyncStatus {
    val json = value.requireObject("sync status")
    return SyncStatus(
        localHeight = json.requireLong("local_height"),
        targetHeight = json.requireLong("target_height"),
        percent = json.requireDouble("percent"),
        eta = json.nullableLong("eta"),
        stage = parseSyncStage(json.requireString("stage")),
        lastCheckpoint = json.nullableLong("last_checkpoint"),
        blocksPerSecond = json.requireDouble("blocks_per_second"),
        notesDecrypted = json.requireLong("notes_decrypted"),
        lastBatchMs = json.requireLong("last_batch_ms"),
    )
}

private fun parseSyncStage(value: String): SyncStage = when (value) {
    "Headers" -> SyncStage.Headers
    "Notes" -> SyncStage.Notes
    "Witness" -> SyncStage.Witness
    "Verify" -> SyncStage.Verify
    else -> throw PirateWalletSdkException("Unknown sync stage: $value")
}

private fun parseTransactionOutput(value: Any?): TransactionOutput {
    val json = value.requireObject("transaction output")
    return TransactionOutput(
        address = json.requireString("addr"),
        amount = json.requireLong("amount"),
        memo = json.nullableString("memo"),
    )
}

private fun parseAddressValidation(value: Any?): AddressValidation {
    val json = value.requireObject("address validation")
    return AddressValidation(
        isValid = json.requireBoolean("is_valid"),
        addressType = when (json.nullableString("address_type")) {
            "Sapling" -> ShieldedAddressType.Sapling
            "Orchard" -> ShieldedAddressType.Orchard
            null -> null
            else -> throw PirateWalletSdkException(
                "Unknown shielded address type: ${json.optString("address_type")}",
            )
        },
        reason = json.nullableString("reason"),
    )
}

private fun parseConsensusBranchValidation(value: Any?): ConsensusBranchValidation {
    val json = value.requireObject("consensus branch validation")
    return ConsensusBranchValidation(
        sdkBranchId = json.nullableString("sdk_branch_id"),
        serverBranchId = json.nullableString("server_branch_id"),
        isValid = json.requireBoolean("is_valid"),
        hasServerBranch = json.requireBoolean("has_server_branch"),
        hasSdkBranch = json.requireBoolean("has_sdk_branch"),
        isServerNewer = json.requireBoolean("is_server_newer"),
        isSdkNewer = json.requireBoolean("is_sdk_newer"),
        errorMessage = json.nullableString("error_message"),
    )
}

private fun parseTransactionRecipient(value: Any?): TransactionRecipient {
    val json = value.requireObject("transaction recipient")
    return TransactionRecipient(
        address = json.requireString("address"),
        pool = json.requireString("pool"),
        amount = json.requireLong("amount"),
        outputIndex = json.requireInt("output_index"),
        memo = json.nullableString("memo"),
    )
}

private fun parseTransactionDetails(value: Any?): TransactionDetails? {
    if (value == null || value == JSONObject.NULL) {
        return null
    }

    val json = value.requireObject("transaction details")
    return TransactionDetails(
        txId = json.requireString("txid"),
        height = json.nullableInt("height"),
        timestamp = json.requireLong("timestamp"),
        amount = json.requireLong("amount"),
        fee = json.requireLong("fee"),
        confirmed = json.requireBoolean("confirmed"),
        memo = json.nullableString("memo"),
        recipients = json.requireArray("recipients").toList(::parseTransactionRecipient),
    )
}

private fun parseNetworkInfo(value: Any?): NetworkInfo {
    val json = value.requireObject("network info")
    return NetworkInfo(
        name = json.requireString("name"),
        coinType = json.requireInt("coin_type"),
        rpcPort = json.requireInt("rpc_port"),
        defaultBirthday = json.requireInt("default_birthday"),
    )
}

private fun parseWatchOnlyCapabilities(value: Any?): WatchOnlyCapabilities {
    val json = value.requireObject("watch-only capabilities")
    return WatchOnlyCapabilities(
        canViewIncoming = json.requireBoolean("can_view_incoming"),
        canViewOutgoing = json.requireBoolean("can_view_outgoing"),
        canSpend = json.requireBoolean("can_spend"),
        canExportSeed = json.requireBoolean("can_export_seed"),
        canGenerateAddresses = json.requireBoolean("can_generate_addresses"),
        isWatchOnly = json.requireBoolean("is_watch_only"),
    )
}

private fun parseKeyTypeInfo(value: Any?): KeyTypeInfo = when (value?.toString()) {
    "Seed" -> KeyTypeInfo.Seed
    "ImportedSpending" -> KeyTypeInfo.ImportedSpending
    "ImportedViewing" -> KeyTypeInfo.ImportedViewing
    else -> throw PirateWalletSdkException("Unknown key type: ${describeJsonValue(value)}")
}

private fun parseKeyGroupInfoList(value: Any?): List<KeyGroupInfo> {
    val array = value.requireArray("key groups")
    return array.toList(::parseKeyGroupInfo)
}

private fun parseKeyGroupInfo(value: Any?): KeyGroupInfo {
    val json = value.requireObject("key group")
    return KeyGroupInfo(
        id = json.requireLong("id"),
        label = json.nullableString("label"),
        keyType = parseKeyTypeInfo(json.opt("key_type")),
        spendable = json.requireBoolean("spendable"),
        hasSapling = json.requireBoolean("has_sapling"),
        hasOrchard = json.requireBoolean("has_orchard"),
        birthdayHeight = json.requireLong("birthday_height"),
        createdAt = json.requireLong("created_at"),
    )
}

private fun parseKeyExportInfo(value: Any?): KeyExportInfo {
    val json = value.requireObject("key export info")
    return KeyExportInfo(
        keyId = json.requireLong("key_id"),
        saplingViewingKey = json.nullableString("sapling_viewing_key"),
        orchardViewingKey = json.nullableString("orchard_viewing_key"),
        saplingSpendingKey = json.nullableString("sapling_spending_key"),
        orchardSpendingKey = json.nullableString("orchard_spending_key"),
    )
}

private fun parseBoolean(value: Any?): Boolean = when (value) {
    is Boolean -> value
    is Number -> value.toInt() != 0
    is String -> value.toBooleanStrictOrNull()
        ?: throw PirateWalletSdkException("Expected boolean value, got '$value'")
    else -> throw PirateWalletSdkException("Expected boolean value, got ${describeJsonValue(value)}")
}

private fun parseIntValue(value: Any?): Int = when (value) {
    is Number -> value.toInt()
    is String -> value.toIntOrNull()
        ?: throw PirateWalletSdkException("Expected int value, got '$value'")
    else -> throw PirateWalletSdkException("Expected int value, got ${describeJsonValue(value)}")
}

private fun parseLongValue(value: Any?): Long = when (value) {
    is Number -> value.toLong()
    is String -> value.toLongOrNull()
        ?: throw PirateWalletSdkException("Expected long value, got '$value'")
    else -> throw PirateWalletSdkException("Expected long value, got ${describeJsonValue(value)}")
}

private fun parseString(value: Any?): String = when (value) {
    is String -> value
    else -> throw PirateWalletSdkException("Expected string value, got ${describeJsonValue(value)}")
}

private fun parseStringOrNull(value: Any?): String? = when (value) {
    null, JSONObject.NULL -> null
    is String -> value
    else -> throw PirateWalletSdkException("Expected nullable string value, got ${describeJsonValue(value)}")
}

private fun parseLongOrNull(value: Any?): Long? = when (value) {
    null, JSONObject.NULL -> null
    else -> parseLongValue(value)
}

private fun parseStringList(value: Any?): List<String> {
    val array = value.requireArray("string list")
    return array.toList(::parseString)
}

private fun Any?.requireObject(label: String): JSONObject = when (this) {
    is JSONObject -> this
    else -> throw PirateWalletSdkException("Expected $label object, got ${describeJsonValue(this)}")
}

private fun JSONObject.requireObject(name: String): JSONObject = when {
    has(name) && !isNull(name) -> opt(name).requireObject("field '$name'")
    else -> throw PirateWalletSdkException("Missing required object field '$name'")
}

private fun Any?.requireArray(label: String): JSONArray = when (this) {
    is JSONArray -> this
    else -> throw PirateWalletSdkException("Expected $label array, got ${describeJsonValue(this)}")
}

private fun JSONObject.requireArray(name: String): JSONArray = when {
    has(name) && !isNull(name) -> opt(name).requireArray("field '$name'")
    else -> throw PirateWalletSdkException("Missing required array field '$name'")
}

private fun <T> JSONArray.toList(transform: (Any?) -> T): List<T> {
    val result = ArrayList<T>(length())
    for (index in 0 until length()) {
        result += transform(get(index))
    }
    return result
}

private fun JSONArray.toByteArray(): ByteArray {
    val bytes = ByteArray(length())
    for (index in 0 until length()) {
        bytes[index] = getInt(index).toByte()
    }
    return bytes
}

private fun Any?.nullableString(): String? = when (this) {
    null, JSONObject.NULL -> null
    is String -> this
    else -> throw PirateWalletSdkException("Expected nullable string value, got ${describeJsonValue(this)}")
}

private fun Any?.nullableLong(): Long? = when (this) {
    null, JSONObject.NULL -> null
    is Number -> this.toLong()
    is String -> this.toLongOrNull()
    else -> null
}

private fun Any?.nullableInt(): Int? = when (this) {
    null, JSONObject.NULL -> null
    is Number -> this.toInt()
    is String -> this.toIntOrNull()
    else -> null
}

private fun Any?.nullableBoolean(): Boolean? = when (this) {
    null, JSONObject.NULL -> null
    is Boolean -> this
    is Number -> this.toInt() != 0
    is String -> this.toBooleanStrictOrNull()
    else -> null
}

private fun JSONObject.requireString(name: String): String = when {
    has(name) && !isNull(name) -> getString(name)
    else -> throw PirateWalletSdkException("Missing required string field '$name'")
}

private fun JSONObject.requireLong(name: String): Long = when {
    has(name) && !isNull(name) -> getLong(name)
    else -> throw PirateWalletSdkException("Missing required long field '$name'")
}

private fun JSONObject.requireInt(name: String): Int = when {
    has(name) && !isNull(name) -> getInt(name)
    else -> throw PirateWalletSdkException("Missing required int field '$name'")
}

private fun JSONObject.requireDouble(name: String): Double = when {
    has(name) && !isNull(name) -> getDouble(name)
    else -> throw PirateWalletSdkException("Missing required double field '$name'")
}

private fun JSONObject.requireBoolean(name: String): Boolean = when {
    has(name) && !isNull(name) -> getBoolean(name)
    else -> throw PirateWalletSdkException("Missing required boolean field '$name'")
}

private fun JSONObject.nullableString(name: String): String? =
    if (has(name) && !isNull(name)) opt(name).nullableString() else null

private fun JSONObject.nullableLong(name: String): Long? =
    if (has(name) && !isNull(name)) opt(name).nullableLong() else null

private fun JSONObject.nullableInt(name: String): Int? =
    if (has(name) && !isNull(name)) opt(name).nullableInt() else null

private fun JSONObject.nullableBoolean(name: String): Boolean? =
    if (has(name) && !isNull(name)) opt(name).nullableBoolean() else null

private fun toJsonCompatible(value: Any?): Any = when (value) {
    null -> JSONObject.NULL
    is JSONObject -> value
    is JSONArray -> value
    is String, is Boolean, is Int, is Long, is Double, is Float -> value
    is Short -> value.toInt()
    is Byte -> value.toInt()
    is ByteArray -> JSONArray().apply {
        for (byte in value) {
            put(byte.toInt() and 0xff)
        }
    }
    is Enum<*> -> value.name
    is TransactionOutput -> value.toJson()
    is PendingTransaction -> value.toJson()
    is SignedTransaction -> value.toJson()
    is BuildTransactionRequest -> value.toJson()
    is CreateWalletRequest -> value.toJson()
    is RestoreWalletRequest -> value.toJson()
    is ImportViewingWalletRequest -> value.toJson()
    is SyncRequest -> value.toJson()
    is RescanRequest -> value.toJson()
    is Iterable<*> -> JSONArray().apply {
        for (element in value) {
            put(toJsonCompatible(element))
        }
    }
    is Array<*> -> JSONArray().apply {
        for (element in value) {
            put(toJsonCompatible(element))
        }
    }
    else -> value
}

private fun TransactionOutput.toJson(): JSONObject =
    JSONObject()
        .put("addr", address)
        .put("amount", amount)
        .apply {
            memo?.let { put("memo", it) }
        }

private fun PendingTransaction.toJson(): JSONObject =
    JSONObject()
        .put("id", id)
        .put("outputs", JSONArray().apply { outputs.forEach { put(it.toJson()) } })
        .put("total_amount", totalAmount)
        .put("fee", fee)
        .put("change", change)
        .put("input_total", inputTotal)
        .put("num_inputs", numInputs)
        .put("expiry_height", expiryHeight)
        .put("created_at", createdAt)

private fun SignedTransaction.toJson(): JSONObject =
    JSONObject()
        .put("txid", txId)
        .put("raw", JSONArray().apply { raw.forEach { put(it.toInt() and 0xff) } })
        .put("size", size)

private fun BuildTransactionRequest.toJson(): JSONObject =
    JSONObject()
        .put("wallet_id", walletId)
        .put("outputs", JSONArray().apply { outputs.forEach { put(it.toJson()) } })
        .apply {
            fee?.let { put("fee_opt", it) }
        }

private fun CreateWalletRequest.toJson(): JSONObject =
    JSONObject()
        .put("name", name)
        .apply {
            birthdayHeight?.let { put("birthday_opt", it) }
        }

private fun RestoreWalletRequest.toJson(): JSONObject =
    JSONObject()
        .put("name", name)
        .put("mnemonic", mnemonic)
        .apply {
            passphrase?.let { put("passphrase_opt", it) }
            birthdayHeight?.let { put("birthday_opt", it) }
        }

private fun ImportViewingWalletRequest.toJson(): JSONObject =
    JSONObject()
        .put("name", name)
        .apply {
            saplingViewingKey?.let { put("sapling_viewing_key", it) }
            orchardViewingKey?.let { put("orchard_viewing_key", it) }
            put("birthday", birthdayHeight)
        }

private fun SyncRequest.toJson(): JSONObject =
    JSONObject()
        .put("wallet_id", walletId)
        .put("mode", mode.name)

private fun RescanRequest.toJson(): JSONObject =
    JSONObject()
        .put("wallet_id", walletId)
        .put("from_height", fromHeight)

private fun describeJsonValue(value: Any?): String = when (value) {
    null, JSONObject.NULL -> "null"
    is JSONObject -> "object"
    is JSONArray -> "array"
    else -> value.javaClass.simpleName
}
