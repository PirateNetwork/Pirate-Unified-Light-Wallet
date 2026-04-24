import Foundation

public enum PirateWalletSdkError: Error, LocalizedError {
    case invalidUtf8
    case nullResponse
    case invalidJsonResponse
    case invalidEnvelope
    case serviceFailure(String)
    case typeMismatch(String)
    case encodingFailed(String)

    public var errorDescription: String? {
        switch self {
        case .invalidUtf8:
            return "Request string was not valid UTF-8."
        case .nullResponse:
            return "Wallet service returned a null response."
        case .invalidJsonResponse:
            return "Wallet service returned invalid JSON."
        case .invalidEnvelope:
            return "Wallet service response envelope was invalid."
        case let .serviceFailure(message):
            return message
        case let .typeMismatch(message):
            return message
        case let .encodingFailed(message):
            return message
        }
    }
}

public enum NetworkType: String, Codable {
    case mainnet = "mainnet"
    case testnet = "testnet"
    case regtest = "regtest"
}

public enum SyncMode: String, Codable {
    case compact = "Compact"
    case deep = "Deep"
}

public enum SyncStage: String, Codable {
    case headers = "Headers"
    case notes = "Notes"
    case witness = "Witness"
    case verify = "Verify"

    public func stageName() -> String {
        switch self {
        case .headers:
            return "Fetching Headers"
        case .notes:
            return "Scanning Notes"
        case .witness:
            return "Building Witnesses"
        case .verify:
            return "Synching Chain"
        }
    }
}

public enum ShieldedAddressType: String, Codable {
    case sapling = "Sapling"
    case orchard = "Orchard"
}

public enum KeyTypeInfo: String, Codable {
    case seed = "Seed"
    case importedSpending = "ImportedSpending"
    case importedViewing = "ImportedViewing"
}

public struct BuildInfo: Codable, Equatable {
    public let version: String
    public let gitCommit: String
    public let buildDate: String
    public let rustVersion: String
    public let targetTriple: String
}

public struct WalletMeta: Codable, Equatable {
    public let id: String
    public let name: String
    public let createdAt: Int64
    public let watchOnly: Bool
    public let birthdayHeight: Int
    public let networkType: NetworkType?
}

public enum MnemonicLanguage: String, Codable, Equatable {
    case english = "english"
    case chineseSimplified = "chinese_simplified"
    case chineseTraditional = "chinese_traditional"
    case french = "french"
    case italian = "italian"
    case japanese = "japanese"
    case korean = "korean"
    case spanish = "spanish"
}

public struct MnemonicInspection: Codable, Equatable {
    public let isValid: Bool
    public let detectedLanguage: MnemonicLanguage?
    public let ambiguousLanguages: [MnemonicLanguage]
    public let wordCount: Int
}

public struct CreateWalletRequest: Codable, Equatable {
    public let name: String
    public let birthdayHeight: Int?
    public let mnemonicLanguage: MnemonicLanguage?

    public init(
        name: String,
        birthdayHeight: Int? = nil,
        mnemonicLanguage: MnemonicLanguage? = nil
    ) {
        self.name = name
        self.birthdayHeight = birthdayHeight
        self.mnemonicLanguage = mnemonicLanguage
    }
}

public struct RestoreWalletRequest: Codable, Equatable {
    public let name: String
    public let mnemonic: String
    public let birthdayHeight: Int?
    public let mnemonicLanguage: MnemonicLanguage?

    public init(
        name: String,
        mnemonic: String,
        birthdayHeight: Int? = nil,
        mnemonicLanguage: MnemonicLanguage? = nil
    ) {
        self.name = name
        self.mnemonic = mnemonic
        self.birthdayHeight = birthdayHeight
        self.mnemonicLanguage = mnemonicLanguage
    }
}

public struct ImportViewingWalletRequest: Codable, Equatable {
    public let name: String
    public let saplingViewingKey: String?
    public let orchardViewingKey: String?
    public let birthdayHeight: Int

    public init(
        name: String,
        saplingViewingKey: String? = nil,
        orchardViewingKey: String? = nil,
        birthdayHeight: Int
    ) {
        self.name = name
        self.saplingViewingKey = saplingViewingKey
        self.orchardViewingKey = orchardViewingKey
        self.birthdayHeight = birthdayHeight
    }
}

public struct ImportWatchOnlyWalletRequest: Codable, Equatable {
    public let name: String
    public let saplingViewingKey: String
    public let birthdayHeight: Int

    public init(name: String, saplingViewingKey: String, birthdayHeight: Int) {
        self.name = name
        self.saplingViewingKey = saplingViewingKey
        self.birthdayHeight = birthdayHeight
    }
}

public struct ImportSpendingKeyRequest: Codable, Equatable {
    public let walletId: String
    public let saplingSpendingKey: String?
    public let orchardSpendingKey: String?
    public let birthdayHeight: Int

    public init(
        walletId: String,
        saplingSpendingKey: String? = nil,
        orchardSpendingKey: String? = nil,
        birthdayHeight: Int
    ) {
        self.walletId = walletId
        self.saplingSpendingKey = saplingSpendingKey
        self.orchardSpendingKey = orchardSpendingKey
        self.birthdayHeight = birthdayHeight
    }
}

public struct TransactionOutput: Codable, Equatable {
    public let address: String
    public let amount: Int64
    public let memo: String?

    public init(address: String, amount: Int64, memo: String? = nil) {
        self.address = address
        self.amount = amount
        self.memo = memo
    }
}

public struct BuildTransactionRequest: Codable, Equatable {
    public let walletId: String
    public let outputs: [TransactionOutput]
    public let fee: Int64?

    public init(walletId: String, outputs: [TransactionOutput], fee: Int64? = nil) {
        self.walletId = walletId
        self.outputs = outputs
        self.fee = fee
    }
}

public struct SyncRequest: Codable, Equatable {
    public let walletId: String
    public let mode: SyncMode

    public init(walletId: String, mode: SyncMode = .compact) {
        self.walletId = walletId
        self.mode = mode
    }
}

public struct RescanRequest: Codable, Equatable {
    public let walletId: String
    public let fromHeight: Int

    public init(walletId: String, fromHeight: Int) {
        self.walletId = walletId
        self.fromHeight = fromHeight
    }
}

public struct Balance: Codable, Equatable {
    public let total: Int64
    public let spendable: Int64
    public let pending: Int64
}

public struct ShieldedPoolBalances: Codable, Equatable {
    public let sapling: Balance
    public let orchard: Balance
}

public struct TransactionInfo: Codable, Equatable {
    public let txId: String
    public let height: Int?
    public let timestamp: Int64
    public let amount: Int64
    public let fee: Int64
    public let memo: String?
    public let confirmed: Bool
}

public struct PendingTransaction: Codable, Equatable {
    public let id: String
    public let outputs: [TransactionOutput]
    public let totalAmount: Int64
    public let fee: Int64
    public let change: Int64
    public let inputTotal: Int64
    public let numInputs: Int
    public let expiryHeight: Int
    public let createdAt: Int64

    public var totalSendValue: Int64 {
        totalAmount + fee
    }

    public var hasMemo: Bool {
        outputs.contains { $0.memo != nil }
    }
}

public struct SignedTransaction: Codable, Equatable {
    public let txId: String
    public let raw: Data
    public let size: Int

    public init(txId: String, raw: Data, size: Int) {
        self.txId = txId
        self.raw = raw
        self.size = size
    }

    public func rawHex() -> String {
        raw.map { String(format: "%02x", $0) }.joined()
    }

    enum CodingKeys: String, CodingKey {
        case txId
        case raw
        case size
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        txId = try container.decode(String.self, forKey: .txId)
        let bytes = try container.decode([UInt8].self, forKey: .raw)
        raw = Data(bytes)
        size = try container.decode(Int.self, forKey: .size)
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(txId, forKey: .txId)
        try container.encode([UInt8](raw), forKey: .raw)
        try container.encode(size, forKey: .size)
    }
}

public struct FeeInfo: Codable, Equatable {
    public let defaultFee: Int64
    public let minFee: Int64
    public let maxFee: Int64
    public let feePerOutput: Int64
    public let memoFeeMultiplier: Double
}

public struct SyncStatus: Codable, Equatable {
    public let localHeight: Int64
    public let targetHeight: Int64
    public let percent: Double
    public let eta: Int64?
    public let stage: SyncStage
    public let lastCheckpoint: Int64?
    public let blocksPerSecond: Double
    public let notesDecrypted: Int64
    public let lastBatchMs: Int64

    public func isSyncing() -> Bool {
        localHeight < targetHeight && targetHeight > 0
    }

    public func isComplete() -> Bool {
        localHeight >= targetHeight && targetHeight > 0
    }

    public func etaFormatted() -> String {
        guard let eta else {
            return "Calculating..."
        }
        if eta > 3600 {
            return "\(eta / 3600)h \((eta % 3600) / 60)m"
        }
        if eta > 60 {
            return "\(eta / 60)m \(eta % 60)s"
        }
        return "\(eta)s"
    }
}

public struct CheckpointInfo: Codable, Equatable {
    public let height: Int
    public let timestamp: Int64
}

public struct AddressInfo: Codable, Equatable {
    public let address: String
    public let diversifierIndex: Int
    public let createdAt: Int64
}

public struct AddressBalanceInfo: Codable, Equatable {
    public let address: String
    public let balance: Int64
    public let spendable: Int64
    public let pending: Int64
    public let keyId: Int64?
    public let addressId: Int64
    public let createdAt: Int64
    public let diversifierIndex: Int
}

public struct SpendabilityStatus: Codable, Equatable {
    public let spendable: Bool
    public let rescanRequired: Bool
    public let targetHeight: Int64
    public let anchorHeight: Int64
    public let validatedAnchorHeight: Int64
    public let repairQueued: Bool
    public let reasonCode: String

    public func isReadyToSpend() -> Bool {
        spendable
    }
}

public struct NetworkInfo: Codable, Equatable {
    public let name: String
    public let coinType: Int
    public let rpcPort: Int
    public let defaultBirthday: Int
}

public struct WatchOnlyCapabilities: Codable, Equatable {
    public let canViewIncoming: Bool
    public let canViewOutgoing: Bool
    public let canSpend: Bool
    public let canExportSeed: Bool
    public let canGenerateAddresses: Bool
    public let isWatchOnly: Bool
}

public struct KeyGroupInfo: Codable, Equatable {
    public let id: Int64
    public let keyType: KeyTypeInfo
    public let spendable: Bool
    public let hasSapling: Bool
    public let hasOrchard: Bool
    public let birthdayHeight: Int64
    public let createdAt: Int64
}

public struct KeyExportInfo: Codable, Equatable {
    public let keyId: Int64
    public let saplingViewingKey: String?
    public let orchardViewingKey: String?
    public let saplingSpendingKey: String?
    public let orchardSpendingKey: String?
}

public struct AddressValidation: Codable, Equatable {
    public let isValid: Bool
    public let addressType: ShieldedAddressType?
    public let reason: String?

    public func isInvalid() -> Bool {
        !isValid
    }
}

public struct ConsensusBranchValidation: Codable, Equatable {
    public let sdkBranchId: String?
    public let serverBranchId: String?
    public let isValid: Bool
    public let hasServerBranch: Bool
    public let hasSdkBranch: Bool
    public let isServerNewer: Bool
    public let isSdkNewer: Bool
    public let errorMessage: String?
}

public struct TransactionRecipient: Codable, Equatable {
    public let address: String
    public let pool: String
    public let amount: Int64
    public let outputIndex: Int
    public let memo: String?
    public let paymentDisclosure: String?
}

public struct TransactionDetails: Codable, Equatable {
    public let txId: String
    public let height: Int?
    public let timestamp: Int64
    public let amount: Int64
    public let fee: Int64
    public let confirmed: Bool
    public let memo: String?
    public let recipients: [TransactionRecipient]
}

public struct PaymentDisclosure: Codable, Equatable {
    public let disclosureType: String
    public let txId: String
    public let outputIndex: Int
    public let address: String
    public let amount: Int64
    public let memo: String?
    public let disclosure: String
}

public struct PaymentDisclosureVerification: Codable, Equatable {
    public let disclosureType: String
    public let txId: String
    public let outputIndex: Int
    public let address: String
    public let amount: Int64
    public let memo: String?
    public let memoHex: String
}
