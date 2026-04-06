import Foundation
import PirateWalletNative

public protocol PirateWalletNativeInvoker {
    func invoke(requestJson: String, pretty: Bool) throws -> String
}

public struct PirateWalletCInvoker: PirateWalletNativeInvoker {
    public init() {}

    public func invoke(requestJson: String, pretty: Bool) throws -> String {
        guard let request = requestJson.cString(using: .utf8) else {
            throw PirateWalletSdkError.invalidUtf8
        }

        let pointer = request.withUnsafeBufferPointer { buffer in
            pirate_wallet_service_invoke_json(buffer.baseAddress, pretty)
        }

        guard let pointer else {
            throw PirateWalletSdkError.nullResponse
        }
        defer {
            pirate_wallet_service_free_string(pointer)
        }

        return String(cString: pointer)
    }
}

public final class PirateWalletSDK {
    private let invoker: PirateWalletNativeInvoker
    private let invocationQueue = DispatchQueue(
        label: "com.pirate.wallet.sdk.invoke",
        qos: .userInitiated
    )
    public lazy var advancedKeyManagement: PirateWalletAdvancedKeyManagement = PirateWalletAdvancedKeyManagement(sdk: self)

    public init(invoker: PirateWalletNativeInvoker = PirateWalletCInvoker()) {
        self.invoker = invoker
    }

    public func invoke(requestJson: String, pretty: Bool = false) throws -> String {
        try invoker.invoke(requestJson: requestJson, pretty: pretty)
    }

    public func invokeAsync(requestJson: String, pretty: Bool = false) async throws -> String {
        try await withCheckedThrowingContinuation { continuation in
            invocationQueue.async { [invoker] in
                do {
                    continuation.resume(
                        returning: try invoker.invoke(requestJson: requestJson, pretty: pretty)
                    )
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func createSynchronizer(
        walletId: String,
        config: PirateWalletSynchronizer.Config = .init()
    ) -> PirateWalletSynchronizer {
        PirateWalletSynchronizer(sdk: self, walletId: walletId, config: config)
    }

    public func buildInfoJson(pretty: Bool = false) throws -> String {
        try invoke(requestJson: #"{"method":"get_build_info"}"#, pretty: pretty)
    }

    public func buildInfo() throws -> BuildInfo {
        try decodeResult("get_build_info", as: BuildInfo.self)
    }

    public func walletRegistryExists() throws -> Bool {
        try boolResult("wallet_registry_exists")
    }

    public func listWallets() throws -> [WalletMeta] {
        try decodeResult("list_wallets", as: [WalletMeta].self)
    }

    public func getActiveWalletId() throws -> String? {
        try optionalStringResult("get_active_wallet")
    }

    public func getActiveWallet() throws -> WalletMeta? {
        guard let activeWalletId = try getActiveWalletId() else {
            return nil
        }
        return try getWallet(walletId: activeWalletId)
    }

    public func getWallet(walletId: String) throws -> WalletMeta? {
        try listWallets().first { $0.id == walletId }
    }

    public func createWallet(request: CreateWalletRequest) throws -> String {
        try stringResult(
            "create_wallet",
            params: [
                "name": request.name,
                "birthday_opt": request.birthdayHeight,
            ]
        )
    }

    public func createWallet(name: String, birthdayHeight: Int? = nil) throws -> String {
        try createWallet(request: CreateWalletRequest(name: name, birthdayHeight: birthdayHeight))
    }

    public func restoreWallet(request: RestoreWalletRequest) throws -> String {
        try stringResult(
            "restore_wallet",
            params: [
                "name": request.name,
                "mnemonic": request.mnemonic,
                "birthday_opt": request.birthdayHeight,
            ]
        )
    }

    public func restoreWallet(
        name: String,
        mnemonic: String,
        birthdayHeight: Int? = nil
    ) throws -> String {
        try restoreWallet(
            request: RestoreWalletRequest(
                name: name,
                mnemonic: mnemonic,
                birthdayHeight: birthdayHeight
            )
        )
    }

    public func importViewingWallet(request: ImportViewingWalletRequest) throws -> String {
        try stringResult(
            "import_viewing_wallet",
            params: [
                "name": request.name,
                "sapling_viewing_key": request.saplingViewingKey,
                "orchard_viewing_key": request.orchardViewingKey,
                "birthday": request.birthdayHeight,
            ]
        )
    }

    public func importViewingWallet(
        name: String,
        saplingViewingKey: String? = nil,
        orchardViewingKey: String? = nil,
        birthdayHeight: Int
    ) throws -> String {
        try importViewingWallet(
            request: ImportViewingWalletRequest(
                name: name,
                saplingViewingKey: saplingViewingKey,
                orchardViewingKey: orchardViewingKey,
                birthdayHeight: birthdayHeight
            )
        )
    }

    public func switchWallet(walletId: String) throws {
        _ = try invokeResult("switch_wallet", params: ["wallet_id": walletId])
    }

    public func renameWallet(walletId: String, newName: String) throws {
        _ = try invokeResult("rename_wallet", params: ["wallet_id": walletId, "new_name": newName])
    }

    public func deleteWallet(walletId: String) throws {
        _ = try invokeResult("delete_wallet", params: ["wallet_id": walletId])
    }

    public func setWalletBirthdayHeight(walletId: String, birthdayHeight: Int) throws {
        _ = try invokeResult(
            "set_wallet_birthday_height",
            params: ["wallet_id": walletId, "birthday_height": birthdayHeight]
        )
    }

    public func getLatestBirthdayHeight(walletId: String) throws -> Int? {
        try getWallet(walletId: walletId)?.birthdayHeight
    }

    public func generateMnemonic(wordCount: Int? = nil) throws -> String {
        try stringResult("generate_mnemonic", params: ["word_count": wordCount])
    }

    public func validateMnemonic(_ mnemonic: String) throws -> Bool {
        try boolResult("validate_mnemonic", params: ["mnemonic": mnemonic])
    }

    public func getNetworkInfo() throws -> NetworkInfo {
        try decodeResult("get_network_info", as: NetworkInfo.self)
    }

    public func isValidShieldedAddr(_ address: String) throws -> Bool {
        try boolResult("is_valid_shielded_address", params: ["address": address])
    }

    public func validateAddress(_ address: String) throws -> AddressValidation {
        try decodeResult("validate_address", params: ["address": address], as: AddressValidation.self)
    }

    public func validateConsensusBranch(walletId: String) throws -> ConsensusBranchValidation {
        try decodeResult(
            "validate_consensus_branch",
            params: ["wallet_id": walletId],
            as: ConsensusBranchValidation.self
        )
    }

    public func formatAmount(_ arrrtoshis: Int64) throws -> String {
        try stringResult("format_amount", params: ["arrrtoshis": arrrtoshis])
    }

    public func parseAmount(_ arrr: String) throws -> Int64 {
        try int64Result("parse_amount", params: ["arrr": arrr])
    }

    public func getCurrentReceiveAddress(walletId: String) throws -> String {
        try getCurrentAddress(walletId: walletId)
    }

    public func getCurrentAddress(walletId: String) throws -> String {
        try stringResult("current_receive_address", params: ["wallet_id": walletId])
    }

    public func getNextReceiveAddress(walletId: String) throws -> String {
        try getNextAddress(walletId: walletId)
    }

    public func getNextAddress(walletId: String) throws -> String {
        try stringResult("next_receive_address", params: ["wallet_id": walletId])
    }

    public func listAddresses(walletId: String) throws -> [AddressInfo] {
        try decodeResult("list_addresses", params: ["wallet_id": walletId], as: [AddressInfo].self)
    }

    public func listAddressBalances(walletId: String, keyId: Int64? = nil) throws -> [AddressBalanceInfo] {
        try decodeResult(
            "list_address_balances",
            params: ["wallet_id": walletId, "key_id": keyId],
            as: [AddressBalanceInfo].self
        )
    }

    public func getBalance(walletId: String) throws -> Balance {
        try decodeResult("get_balance", params: ["wallet_id": walletId], as: Balance.self)
    }

    public func getShieldedPoolBalances(walletId: String) throws -> ShieldedPoolBalances {
        try decodeResult(
            "get_shielded_pool_balances",
            params: ["wallet_id": walletId],
            as: ShieldedPoolBalances.self
        )
    }

    public func getSpendabilityStatus(walletId: String) throws -> SpendabilityStatus {
        try decodeResult(
            "get_spendability_status",
            params: ["wallet_id": walletId],
            as: SpendabilityStatus.self
        )
    }

    public func listTransactions(walletId: String, limit: Int? = nil) throws -> [TransactionInfo] {
        try decodeResult(
            "list_transactions",
            params: ["wallet_id": walletId, "limit": limit],
            as: [TransactionInfo].self
        )
    }

    public func fetchTransactionMemo(
        walletId: String,
        txId: String,
        outputIndex: Int? = nil
    ) throws -> String? {
        try optionalStringResult(
            "fetch_transaction_memo",
            params: [
                "wallet_id": walletId,
                "txid": txId,
                "output_index": outputIndex,
            ]
        )
    }

    public func getTransactionDetails(walletId: String, txId: String) throws -> TransactionDetails? {
        try decodeOptionalResult(
            "get_transaction_details",
            params: ["wallet_id": walletId, "txid": txId],
            as: TransactionDetails.self
        )
    }

    public func getFeeInfo() throws -> FeeInfo {
        try decodeResult("get_fee_info", as: FeeInfo.self)
    }

    public func startSync(request: SyncRequest) throws {
        _ = try invokeResult(
            "start_sync",
            params: ["wallet_id": request.walletId, "mode": request.mode.rawValue]
        )
    }

    public func startSync(walletId: String, mode: SyncMode = .compact) throws {
        try startSync(request: SyncRequest(walletId: walletId, mode: mode))
    }

    public func getSyncStatus(walletId: String) throws -> SyncStatus {
        try decodeResult("sync_status", params: ["wallet_id": walletId], as: SyncStatus.self)
    }

    public func cancelSync(walletId: String) throws {
        _ = try invokeResult("cancel_sync", params: ["wallet_id": walletId])
    }

    public func rescan(request: RescanRequest) throws {
        _ = try invokeResult(
            "rescan",
            params: ["wallet_id": request.walletId, "from_height": request.fromHeight]
        )
    }

    public func rescan(walletId: String, fromHeight: Int) throws {
        try rescan(request: RescanRequest(walletId: walletId, fromHeight: fromHeight))
    }

    public func buildTransaction(request: BuildTransactionRequest) throws -> PendingTransaction {
        try decodeResult(
            "build_tx",
            params: [
                "wallet_id": request.walletId,
                "outputs": try encodableJSONObject(request.outputs),
                "fee_opt": request.fee,
            ],
            as: PendingTransaction.self
        )
    }

    public func buildTransaction(
        walletId: String,
        outputs: [TransactionOutput],
        fee: Int64? = nil
    ) throws -> PendingTransaction {
        try buildTransaction(
            request: BuildTransactionRequest(walletId: walletId, outputs: outputs, fee: fee)
        )
    }

    public func buildTransaction(
        walletId: String,
        output: TransactionOutput,
        fee: Int64? = nil
    ) throws -> PendingTransaction {
        try buildTransaction(walletId: walletId, outputs: [output], fee: fee)
    }

    public func signTransaction(walletId: String, pending: PendingTransaction) throws -> SignedTransaction {
        try decodeResult(
            "sign_tx",
            params: [
                "wallet_id": walletId,
                "pending": try encodableJSONObject(pending),
            ],
            as: SignedTransaction.self
        )
    }

    public func broadcastTransaction(_ signed: SignedTransaction) throws -> String {
        try stringResult("broadcast_tx", params: ["signed": try encodableJSONObject(signed)])
    }

    public func send(
        walletId: String,
        outputs: [TransactionOutput],
        fee: Int64? = nil
    ) throws -> String {
        let signed = try signTransaction(walletId: walletId, pending: buildTransaction(walletId: walletId, outputs: outputs, fee: fee))
        return try broadcastTransaction(signed)
    }

    public func send(
        walletId: String,
        output: TransactionOutput,
        fee: Int64? = nil
    ) throws -> String {
        try send(walletId: walletId, outputs: [output], fee: fee)
    }

    public func exportSaplingViewingKey(walletId: String) throws -> String {
        try stringResult("export_sapling_viewing_key", params: ["wallet_id": walletId])
    }

    public func exportOrchardViewingKey(walletId: String) throws -> String {
        try stringResult("export_orchard_viewing_key", params: ["wallet_id": walletId])
    }

    public func importSaplingViewingKeyAsWatchOnly(
        request: ImportWatchOnlyWalletRequest
    ) throws -> String {
        try stringResult(
            "import_sapling_viewing_key_as_watch_only",
            params: [
                "name": request.name,
                "sapling_viewing_key": request.saplingViewingKey,
                "birthday_height": request.birthdayHeight,
            ]
        )
    }

    public func importSaplingViewingKeyAsWatchOnly(
        name: String,
        saplingViewingKey: String,
        birthdayHeight: Int
    ) throws -> String {
        try importSaplingViewingKeyAsWatchOnly(
            request: ImportWatchOnlyWalletRequest(
                name: name,
                saplingViewingKey: saplingViewingKey,
                birthdayHeight: birthdayHeight
            )
        )
    }

    public func getWatchOnlyCapabilities(walletId: String) throws -> WatchOnlyCapabilities {
        try decodeResult(
            "get_watch_only_capabilities",
            params: ["wallet_id": walletId],
            as: WatchOnlyCapabilities.self
        )
    }

    fileprivate func invokeResult(
        _ method: String,
        params: [String: Any?] = [:]
    ) throws -> Any? {
        let requestJson = try buildRequestJson(method: method, params: params)
        let responseJson = try invoke(requestJson: requestJson, pretty: false)
        let envelope = try parseEnvelope(responseJson)

        if let ok = envelope["ok"] as? Bool, ok {
            if envelope["result"] is NSNull {
                return nil
            }
            return envelope["result"]
        }

        let errorMessage = envelope["error"] as? String ?? "Wallet service request failed"
        throw PirateWalletSdkError.serviceFailure(errorMessage)
    }

    fileprivate func invokeResultAsync(
        _ method: String,
        params: [String: Any?] = [:]
    ) async throws -> Any? {
        let requestJson = try buildRequestJson(method: method, params: params)
        let responseJson = try await invokeAsync(requestJson: requestJson, pretty: false)
        let envelope = try parseEnvelope(responseJson)

        if let ok = envelope["ok"] as? Bool, ok {
            if envelope["result"] is NSNull {
                return nil
            }
            return envelope["result"]
        }

        let errorMessage = envelope["error"] as? String ?? "Wallet service request failed"
        throw PirateWalletSdkError.serviceFailure(errorMessage)
    }
}

public final class PirateWalletAdvancedKeyManagement {
    private unowned let sdk: PirateWalletSDK

    fileprivate init(sdk: PirateWalletSDK) {
        self.sdk = sdk
    }

    public func listKeyGroups(walletId: String) throws -> [KeyGroupInfo] {
        try sdk.decodeResult("list_key_groups", params: ["wallet_id": walletId], as: [KeyGroupInfo].self)
    }

    public func exportKeyGroupKeys(walletId: String, keyId: Int64) throws -> KeyExportInfo {
        try sdk.decodeResult(
            "export_key_group_keys",
            params: ["wallet_id": walletId, "key_id": keyId],
            as: KeyExportInfo.self
        )
    }

    public func importSpendingKey(request: ImportSpendingKeyRequest) throws -> Int64 {
        try sdk.int64Result(
            "import_spending_key",
            params: [
                "wallet_id": request.walletId,
                "sapling_key": request.saplingSpendingKey,
                "orchard_key": request.orchardSpendingKey,
                "birthday_height": request.birthdayHeight,
            ]
        )
    }

    public func importSpendingKey(
        walletId: String,
        birthdayHeight: Int,
        saplingSpendingKey: String? = nil,
        orchardSpendingKey: String? = nil
    ) throws -> Int64 {
        try importSpendingKey(
            request: ImportSpendingKeyRequest(
                walletId: walletId,
                saplingSpendingKey: saplingSpendingKey,
                orchardSpendingKey: orchardSpendingKey,
                birthdayHeight: birthdayHeight
            )
        )
    }

    public func exportSeed(walletId: String) throws -> String {
        try sdk.stringResult(
            "export_seed_raw",
            params: ["wallet_id": walletId]
        )
    }
}

fileprivate extension PirateWalletSDK {
    func stringResult(_ method: String, params: [String: Any?] = [:]) throws -> String {
        guard let result = try invokeResult(method, params: params) as? String else {
            throw PirateWalletSdkError.typeMismatch("Expected string result for \(method).")
        }
        return result
    }

    func optionalStringResult(_ method: String, params: [String: Any?] = [:]) throws -> String? {
        guard let result = try invokeResult(method, params: params) else {
            return nil
        }
        guard let string = result as? String else {
            throw PirateWalletSdkError.typeMismatch("Expected optional string result for \(method).")
        }
        return string
    }

    func boolResult(_ method: String, params: [String: Any?] = [:]) throws -> Bool {
        guard let result = try invokeResult(method, params: params) else {
            throw PirateWalletSdkError.typeMismatch("Expected bool result for \(method).")
        }
        if let value = result as? Bool {
            return value
        }
        if let value = result as? NSNumber {
            return value.boolValue
        }
        throw PirateWalletSdkError.typeMismatch("Expected bool result for \(method).")
    }

    func int64Result(_ method: String, params: [String: Any?] = [:]) throws -> Int64 {
        guard let result = try invokeResult(method, params: params) else {
            throw PirateWalletSdkError.typeMismatch("Expected integer result for \(method).")
        }
        if let value = result as? NSNumber {
            return value.int64Value
        }
        if let value = result as? Int64 {
            return value
        }
        if let value = result as? Int {
            return Int64(value)
        }
        throw PirateWalletSdkError.typeMismatch("Expected integer result for \(method).")
    }

    func decodeResult<T: Decodable>(
        _ method: String,
        params: [String: Any?] = [:],
        as type: T.Type
    ) throws -> T {
        guard let result = try invokeResult(method, params: params) else {
            throw PirateWalletSdkError.typeMismatch("Missing result for \(method).")
        }
        return try decode(result, as: type)
    }

    func decodeOptionalResult<T: Decodable>(
        _ method: String,
        params: [String: Any?] = [:],
        as type: T.Type
    ) throws -> T? {
        guard let result = try invokeResult(method, params: params) else {
            return nil
        }
        return try decode(result, as: type)
    }

    func stringResultAsync(_ method: String, params: [String: Any?] = [:]) async throws -> String {
        guard let result = try await invokeResultAsync(method, params: params) as? String else {
            throw PirateWalletSdkError.typeMismatch("Expected string result for \(method).")
        }
        return result
    }

    func optionalStringResultAsync(_ method: String, params: [String: Any?] = [:]) async throws -> String? {
        guard let result = try await invokeResultAsync(method, params: params) else {
            return nil
        }
        guard let string = result as? String else {
            throw PirateWalletSdkError.typeMismatch("Expected optional string result for \(method).")
        }
        return string
    }

    func boolResultAsync(_ method: String, params: [String: Any?] = [:]) async throws -> Bool {
        guard let result = try await invokeResultAsync(method, params: params) else {
            throw PirateWalletSdkError.typeMismatch("Expected bool result for \(method).")
        }
        if let value = result as? Bool {
            return value
        }
        if let value = result as? NSNumber {
            return value.boolValue
        }
        throw PirateWalletSdkError.typeMismatch("Expected bool result for \(method).")
    }

    func int64ResultAsync(_ method: String, params: [String: Any?] = [:]) async throws -> Int64 {
        guard let result = try await invokeResultAsync(method, params: params) else {
            throw PirateWalletSdkError.typeMismatch("Expected integer result for \(method).")
        }
        if let value = result as? NSNumber {
            return value.int64Value
        }
        if let value = result as? Int64 {
            return value
        }
        if let value = result as? Int {
            return Int64(value)
        }
        throw PirateWalletSdkError.typeMismatch("Expected integer result for \(method).")
    }

    func decodeResultAsync<T: Decodable>(
        _ method: String,
        params: [String: Any?] = [:],
        as type: T.Type
    ) async throws -> T {
        guard let result = try await invokeResultAsync(method, params: params) else {
            throw PirateWalletSdkError.typeMismatch("Missing result for \(method).")
        }
        return try decode(result, as: type)
    }

    func decodeOptionalResultAsync<T: Decodable>(
        _ method: String,
        params: [String: Any?] = [:],
        as type: T.Type
    ) async throws -> T? {
        guard let result = try await invokeResultAsync(method, params: params) else {
            return nil
        }
        return try decode(result, as: type)
    }

    func decode<T: Decodable>(_ value: Any, as type: T.Type) throws -> T {
        let data = try JSONSerialization.data(withJSONObject: value, options: [])
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(T.self, from: data)
    }
}

extension PirateWalletSDK {
    public func buildInfoJsonAsync(pretty: Bool = false) async throws -> String {
        try await invokeAsync(requestJson: #"{"method":"get_build_info"}"#, pretty: pretty)
    }

    public func buildInfoAsync() async throws -> BuildInfo {
        try await decodeResultAsync("get_build_info", as: BuildInfo.self)
    }

    public func walletRegistryExistsAsync() async throws -> Bool {
        try await boolResultAsync("wallet_registry_exists")
    }

    public func listWalletsAsync() async throws -> [WalletMeta] {
        try await decodeResultAsync("list_wallets", as: [WalletMeta].self)
    }

    public func getActiveWalletIdAsync() async throws -> String? {
        try await optionalStringResultAsync("get_active_wallet")
    }

    public func getActiveWalletAsync() async throws -> WalletMeta? {
        guard let activeWalletId = try await getActiveWalletIdAsync() else {
            return nil
        }
        return try await getWalletAsync(walletId: activeWalletId)
    }

    public func getWalletAsync(walletId: String) async throws -> WalletMeta? {
        let wallets = try await listWalletsAsync()
        return wallets.first { $0.id == walletId }
    }

    public func createWalletAsync(request: CreateWalletRequest) async throws -> String {
        try await stringResultAsync(
            "create_wallet",
            params: [
                "name": request.name,
                "birthday_opt": request.birthdayHeight,
            ]
        )
    }

    public func createWalletAsync(name: String, birthdayHeight: Int? = nil) async throws -> String {
        try await createWalletAsync(request: CreateWalletRequest(name: name, birthdayHeight: birthdayHeight))
    }

    public func restoreWalletAsync(request: RestoreWalletRequest) async throws -> String {
        try await stringResultAsync(
            "restore_wallet",
            params: [
                "name": request.name,
                "mnemonic": request.mnemonic,
                "birthday_opt": request.birthdayHeight,
            ]
        )
    }

    public func restoreWalletAsync(
        name: String,
        mnemonic: String,
        birthdayHeight: Int? = nil
    ) async throws -> String {
        try await restoreWalletAsync(
            request: RestoreWalletRequest(
                name: name,
                mnemonic: mnemonic,
                birthdayHeight: birthdayHeight
            )
        )
    }

    public func importViewingWalletAsync(request: ImportViewingWalletRequest) async throws -> String {
        try await stringResultAsync(
            "import_viewing_wallet",
            params: [
                "name": request.name,
                "sapling_viewing_key": request.saplingViewingKey,
                "orchard_viewing_key": request.orchardViewingKey,
                "birthday": request.birthdayHeight,
            ]
        )
    }

    public func importViewingWalletAsync(
        name: String,
        saplingViewingKey: String? = nil,
        orchardViewingKey: String? = nil,
        birthdayHeight: Int
    ) async throws -> String {
        try await importViewingWalletAsync(
            request: ImportViewingWalletRequest(
                name: name,
                saplingViewingKey: saplingViewingKey,
                orchardViewingKey: orchardViewingKey,
                birthdayHeight: birthdayHeight
            )
        )
    }

    public func switchWalletAsync(walletId: String) async throws {
        _ = try await invokeResultAsync("switch_wallet", params: ["wallet_id": walletId])
    }

    public func renameWalletAsync(walletId: String, newName: String) async throws {
        _ = try await invokeResultAsync(
            "rename_wallet",
            params: ["wallet_id": walletId, "new_name": newName]
        )
    }

    public func deleteWalletAsync(walletId: String) async throws {
        _ = try await invokeResultAsync("delete_wallet", params: ["wallet_id": walletId])
    }

    public func setWalletBirthdayHeightAsync(walletId: String, birthdayHeight: Int) async throws {
        _ = try await invokeResultAsync(
            "set_wallet_birthday_height",
            params: ["wallet_id": walletId, "birthday_height": birthdayHeight]
        )
    }

    public func getLatestBirthdayHeightAsync(walletId: String) async throws -> Int? {
        let wallet = try await getWalletAsync(walletId: walletId)
        return wallet?.birthdayHeight
    }

    public func generateMnemonicAsync(wordCount: Int? = nil) async throws -> String {
        try await stringResultAsync("generate_mnemonic", params: ["word_count": wordCount])
    }

    public func validateMnemonicAsync(_ mnemonic: String) async throws -> Bool {
        try await boolResultAsync("validate_mnemonic", params: ["mnemonic": mnemonic])
    }

    public func getNetworkInfoAsync() async throws -> NetworkInfo {
        try await decodeResultAsync("get_network_info", as: NetworkInfo.self)
    }

    public func isValidShieldedAddrAsync(_ address: String) async throws -> Bool {
        try await boolResultAsync("is_valid_shielded_address", params: ["address": address])
    }

    public func validateAddressAsync(_ address: String) async throws -> AddressValidation {
        try await decodeResultAsync(
            "validate_address",
            params: ["address": address],
            as: AddressValidation.self
        )
    }

    public func validateConsensusBranchAsync(walletId: String) async throws -> ConsensusBranchValidation {
        try await decodeResultAsync(
            "validate_consensus_branch",
            params: ["wallet_id": walletId],
            as: ConsensusBranchValidation.self
        )
    }

    public func formatAmountAsync(_ arrrtoshis: Int64) async throws -> String {
        try await stringResultAsync("format_amount", params: ["arrrtoshis": arrrtoshis])
    }

    public func parseAmountAsync(_ arrr: String) async throws -> Int64 {
        try await int64ResultAsync("parse_amount", params: ["arrr": arrr])
    }

    public func getCurrentReceiveAddressAsync(walletId: String) async throws -> String {
        try await getCurrentAddressAsync(walletId: walletId)
    }

    public func getCurrentAddressAsync(walletId: String) async throws -> String {
        try await stringResultAsync("current_receive_address", params: ["wallet_id": walletId])
    }

    public func getNextReceiveAddressAsync(walletId: String) async throws -> String {
        try await getNextAddressAsync(walletId: walletId)
    }

    public func getNextAddressAsync(walletId: String) async throws -> String {
        try await stringResultAsync("next_receive_address", params: ["wallet_id": walletId])
    }

    public func listAddressesAsync(walletId: String) async throws -> [AddressInfo] {
        try await decodeResultAsync(
            "list_addresses",
            params: ["wallet_id": walletId],
            as: [AddressInfo].self
        )
    }

    public func listAddressBalancesAsync(walletId: String, keyId: Int64? = nil) async throws -> [AddressBalanceInfo] {
        try await decodeResultAsync(
            "list_address_balances",
            params: ["wallet_id": walletId, "key_id": keyId],
            as: [AddressBalanceInfo].self
        )
    }

    public func getBalanceAsync(walletId: String) async throws -> Balance {
        try await decodeResultAsync(
            "get_balance",
            params: ["wallet_id": walletId],
            as: Balance.self
        )
    }

    public func getShieldedPoolBalancesAsync(walletId: String) async throws -> ShieldedPoolBalances {
        try await decodeResultAsync(
            "get_shielded_pool_balances",
            params: ["wallet_id": walletId],
            as: ShieldedPoolBalances.self
        )
    }

    public func getSpendabilityStatusAsync(walletId: String) async throws -> SpendabilityStatus {
        try await decodeResultAsync(
            "get_spendability_status",
            params: ["wallet_id": walletId],
            as: SpendabilityStatus.self
        )
    }

    public func listTransactionsAsync(walletId: String, limit: Int? = nil) async throws -> [TransactionInfo] {
        try await decodeResultAsync(
            "list_transactions",
            params: ["wallet_id": walletId, "limit": limit],
            as: [TransactionInfo].self
        )
    }

    public func fetchTransactionMemoAsync(
        walletId: String,
        txId: String,
        outputIndex: Int? = nil
    ) async throws -> String? {
        try await optionalStringResultAsync(
            "fetch_transaction_memo",
            params: [
                "wallet_id": walletId,
                "txid": txId,
                "output_index": outputIndex,
            ]
        )
    }

    public func getTransactionDetailsAsync(walletId: String, txId: String) async throws -> TransactionDetails? {
        try await decodeOptionalResultAsync(
            "get_transaction_details",
            params: ["wallet_id": walletId, "txid": txId],
            as: TransactionDetails.self
        )
    }

    public func getFeeInfoAsync() async throws -> FeeInfo {
        try await decodeResultAsync("get_fee_info", as: FeeInfo.self)
    }

    public func startSyncAsync(request: SyncRequest) async throws {
        _ = try await invokeResultAsync(
            "start_sync",
            params: ["wallet_id": request.walletId, "mode": request.mode.rawValue]
        )
    }

    public func startSyncAsync(walletId: String, mode: SyncMode = .compact) async throws {
        _ = try await invokeResultAsync(
            "start_sync",
            params: ["wallet_id": walletId, "mode": mode.rawValue]
        )
    }

    public func getSyncStatusAsync(walletId: String) async throws -> SyncStatus {
        try await decodeResultAsync(
            "sync_status",
            params: ["wallet_id": walletId],
            as: SyncStatus.self
        )
    }

    public func cancelSyncAsync(walletId: String) async throws {
        _ = try await invokeResultAsync("cancel_sync", params: ["wallet_id": walletId])
    }

    public func rescanAsync(request: RescanRequest) async throws {
        _ = try await invokeResultAsync(
            "rescan",
            params: ["wallet_id": request.walletId, "from_height": request.fromHeight]
        )
    }

    public func rescanAsync(walletId: String, fromHeight: Int) async throws {
        try await rescanAsync(request: RescanRequest(walletId: walletId, fromHeight: fromHeight))
    }

    public func buildTransactionAsync(request: BuildTransactionRequest) async throws -> PendingTransaction {
        try await decodeResultAsync(
            "build_tx",
            params: [
                "wallet_id": request.walletId,
                "outputs": try encodableJSONObject(request.outputs),
                "fee_opt": request.fee,
            ],
            as: PendingTransaction.self
        )
    }

    public func buildTransactionAsync(
        walletId: String,
        outputs: [TransactionOutput],
        fee: Int64? = nil
    ) async throws -> PendingTransaction {
        try await buildTransactionAsync(
            request: BuildTransactionRequest(walletId: walletId, outputs: outputs, fee: fee)
        )
    }

    public func buildTransactionAsync(
        walletId: String,
        output: TransactionOutput,
        fee: Int64? = nil
    ) async throws -> PendingTransaction {
        try await buildTransactionAsync(walletId: walletId, outputs: [output], fee: fee)
    }

    public func signTransactionAsync(walletId: String, pending: PendingTransaction) async throws -> SignedTransaction {
        try await decodeResultAsync(
            "sign_tx",
            params: [
                "wallet_id": walletId,
                "pending": try encodableJSONObject(pending),
            ],
            as: SignedTransaction.self
        )
    }

    public func broadcastTransactionAsync(_ signed: SignedTransaction) async throws -> String {
        try await stringResultAsync("broadcast_tx", params: ["signed": try encodableJSONObject(signed)])
    }

    public func sendAsync(
        walletId: String,
        outputs: [TransactionOutput],
        fee: Int64? = nil
    ) async throws -> String {
        let pending = try await buildTransactionAsync(walletId: walletId, outputs: outputs, fee: fee)
        let signed = try await signTransactionAsync(walletId: walletId, pending: pending)
        return try await broadcastTransactionAsync(signed)
    }

    public func sendAsync(
        walletId: String,
        output: TransactionOutput,
        fee: Int64? = nil
    ) async throws -> String {
        try await sendAsync(walletId: walletId, outputs: [output], fee: fee)
    }

    public func exportSaplingViewingKeyAsync(walletId: String) async throws -> String {
        try await stringResultAsync("export_sapling_viewing_key", params: ["wallet_id": walletId])
    }

    public func exportOrchardViewingKeyAsync(walletId: String) async throws -> String {
        try await stringResultAsync("export_orchard_viewing_key", params: ["wallet_id": walletId])
    }

    public func importSaplingViewingKeyAsWatchOnlyAsync(
        request: ImportWatchOnlyWalletRequest
    ) async throws -> String {
        try await stringResultAsync(
            "import_sapling_viewing_key_as_watch_only",
            params: [
                "name": request.name,
                "sapling_viewing_key": request.saplingViewingKey,
                "birthday_height": request.birthdayHeight,
            ]
        )
    }

    public func importSaplingViewingKeyAsWatchOnlyAsync(
        name: String,
        saplingViewingKey: String,
        birthdayHeight: Int
    ) async throws -> String {
        try await importSaplingViewingKeyAsWatchOnlyAsync(
            request: ImportWatchOnlyWalletRequest(
                name: name,
                saplingViewingKey: saplingViewingKey,
                birthdayHeight: birthdayHeight
            )
        )
    }

    public func getWatchOnlyCapabilitiesAsync(walletId: String) async throws -> WatchOnlyCapabilities {
        try await decodeResultAsync(
            "get_watch_only_capabilities",
            params: ["wallet_id": walletId],
            as: WatchOnlyCapabilities.self
        )
    }
}

extension PirateWalletAdvancedKeyManagement {
    public func listKeyGroupsAsync(walletId: String) async throws -> [KeyGroupInfo] {
        try await sdk.decodeResultAsync(
            "list_key_groups",
            params: ["wallet_id": walletId],
            as: [KeyGroupInfo].self
        )
    }

    public func exportKeyGroupKeysAsync(walletId: String, keyId: Int64) async throws -> KeyExportInfo {
        try await sdk.decodeResultAsync(
            "export_key_group_keys",
            params: ["wallet_id": walletId, "key_id": keyId],
            as: KeyExportInfo.self
        )
    }

    public func importSpendingKeyAsync(request: ImportSpendingKeyRequest) async throws -> Int64 {
        try await sdk.int64ResultAsync(
            "import_spending_key",
            params: [
                "wallet_id": request.walletId,
                "sapling_key": request.saplingSpendingKey,
                "orchard_key": request.orchardSpendingKey,
                "birthday_height": request.birthdayHeight,
            ]
        )
    }

    public func importSpendingKeyAsync(
        walletId: String,
        birthdayHeight: Int,
        saplingSpendingKey: String? = nil,
        orchardSpendingKey: String? = nil
    ) async throws -> Int64 {
        try await importSpendingKeyAsync(
            request: ImportSpendingKeyRequest(
                walletId: walletId,
                saplingSpendingKey: saplingSpendingKey,
                orchardSpendingKey: orchardSpendingKey,
                birthdayHeight: birthdayHeight
            )
        )
    }

    public func exportSeedAsync(walletId: String) async throws -> String {
        try await sdk.stringResultAsync(
            "export_seed_raw",
            params: ["wallet_id": walletId]
        )
    }
}

private func buildRequestJson(method: String, params: [String: Any?]) throws -> String {
    var request: [String: Any] = ["method": method]
    for (key, value) in params {
        guard let value else {
            continue
        }
        request[key] = value
    }

    guard JSONSerialization.isValidJSONObject(request) else {
        throw PirateWalletSdkError.encodingFailed("Request for \(method) could not be encoded.")
    }

    let data = try JSONSerialization.data(withJSONObject: request, options: [])
    guard let string = String(data: data, encoding: .utf8) else {
        throw PirateWalletSdkError.invalidUtf8
    }
    return string
}

private func encodableJSONObject<T: Encodable>(_ value: T) throws -> Any {
    let encoder = JSONEncoder()
    encoder.keyEncodingStrategy = .convertToSnakeCase
    let data = try encoder.encode(value)
    return try JSONSerialization.jsonObject(with: data, options: [])
}

private func parseEnvelope(_ responseJson: String) throws -> [String: Any] {
    guard let data = responseJson.data(using: .utf8) else {
        throw PirateWalletSdkError.invalidUtf8
    }
    guard let object = try JSONSerialization.jsonObject(with: data, options: []) as? [String: Any] else {
        throw PirateWalletSdkError.invalidEnvelope
    }
    return object
}
