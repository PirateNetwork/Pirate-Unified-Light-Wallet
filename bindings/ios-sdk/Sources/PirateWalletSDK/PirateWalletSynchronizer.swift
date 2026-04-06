import Combine
import Foundation

@MainActor
public final class PirateWalletSynchronizer: ObservableObject {
    public enum Status {
        case stopped
        case syncing
        case synced
    }

    public struct Config: Equatable {
        public let syncMode: SyncMode
        public let syncingPollIntervalMs: UInt64
        public let syncedPollIntervalMs: UInt64
        public let errorPollIntervalMs: UInt64
        public let transactionLimit: Int?

        public init(
            syncMode: SyncMode = .compact,
            syncingPollIntervalMs: UInt64 = 1_000,
            syncedPollIntervalMs: UInt64 = 5_000,
            errorPollIntervalMs: UInt64 = 5_000,
            transactionLimit: Int? = nil
        ) {
            precondition(syncingPollIntervalMs > 0, "syncingPollIntervalMs must be greater than 0")
            precondition(syncedPollIntervalMs > 0, "syncedPollIntervalMs must be greater than 0")
            precondition(errorPollIntervalMs > 0, "errorPollIntervalMs must be greater than 0")
            precondition(transactionLimit == nil || transactionLimit! > 0, "transactionLimit must be greater than 0 when provided")

            self.syncMode = syncMode
            self.syncingPollIntervalMs = syncingPollIntervalMs
            self.syncedPollIntervalMs = syncedPollIntervalMs
            self.errorPollIntervalMs = errorPollIntervalMs
            self.transactionLimit = transactionLimit
        }
    }

    public struct Snapshot {
        public let walletId: String
        public let status: Status
        public let progressPercent: Double
        public let syncStatus: SyncStatus?
        public let latestBirthdayHeight: Int?
        public let balance: Balance?
        public let transactions: [TransactionInfo]
        public let updatedAtMillis: Int64?
        public let lastError: Error?

        public func isRunning() -> Bool {
            status != .stopped
        }

        public func isSyncing() -> Bool {
            status == .syncing
        }

        public func isComplete() -> Bool {
            syncStatus?.isComplete() == true
        }
    }

    private let sdk: PirateWalletSDK
    public let walletId: String
    public let config: Config

    private var pollTask: Task<Void, Never>?

    @Published public private(set) var status: Status = .stopped
    @Published public private(set) var progress: Double = 0
    @Published public private(set) var syncStatus: SyncStatus?
    @Published public private(set) var latestBirthdayHeight: Int?
    @Published public private(set) var balance: Balance?
    @Published public private(set) var transactions: [TransactionInfo] = []
    @Published public private(set) var lastError: Error?
    @Published public private(set) var snapshot: Snapshot

    public init(sdk: PirateWalletSDK, walletId: String, config: Config = Config()) {
        self.sdk = sdk
        self.walletId = walletId
        self.config = config
        self.snapshot = Snapshot(
            walletId: walletId,
            status: .stopped,
            progressPercent: 0,
            syncStatus: nil,
            latestBirthdayHeight: nil,
            balance: nil,
            transactions: [],
            updatedAtMillis: nil,
            lastError: nil
        )
    }

    public func currentSnapshot() -> Snapshot {
        snapshot
    }

    public func isRunning() -> Bool {
        pollTask != nil
    }

    public func isSyncing() -> Bool {
        status == .syncing
    }

    public func isComplete() -> Bool {
        syncStatus?.isComplete() == true
    }

    @discardableResult
    public func start() -> Task<Void, Never> {
        if pollTask != nil {
            return pollTask!
        }

        publish(
            status: .syncing,
            lastError: nil,
            updatedAtMillis: Self.currentTimeMillis()
        )

        let task = Task { [weak self] in
            guard let self else { return }
            do {
                try await self.sdk.startSyncAsync(
                    walletId: self.walletId,
                    mode: self.config.syncMode
                )
                try await self.pollLoop()
            } catch {
                await MainActor.run {
                    self.publish(
                        status: .stopped,
                        lastError: error,
                        updatedAtMillis: Self.currentTimeMillis()
                    )
                    self.pollTask = nil
                }
            }
        }

        pollTask = task
        return task
    }

    @discardableResult
    public func stop() -> Task<Void, Never> {
        let existingTask = pollTask
        let shouldCancelBackend = existingTask != nil || status != .stopped
        pollTask = nil
        publish(status: .stopped, updatedAtMillis: Self.currentTimeMillis())

        return Task { [weak self] in
            existingTask?.cancel()
            if !shouldCancelBackend {
                return
            }
            guard let self else { return }
            do {
                try await self.sdk.cancelSyncAsync(walletId: self.walletId)
            } catch {
                await MainActor.run {
                    self.publish(
                        status: .stopped,
                        lastError: error,
                        updatedAtMillis: Self.currentTimeMillis()
                    )
                }
            }
        }
    }

    @discardableResult
    public func refresh() -> Task<Void, Never> {
        Task { [weak self] in
            guard let self else { return }
            _ = try? await self.refreshOnce()
        }
    }

    @discardableResult
    public func close() -> Task<Void, Never> {
        stop()
    }

    private func pollLoop() async throws {
        while !Task.isCancelled {
            let nextDelay = try await refreshOnce()
            try await Task.sleep(nanoseconds: nextDelay * 1_000_000)
        }
    }

    private func refreshOnce() async throws -> UInt64 {
        let observedAtMillis = Self.currentTimeMillis()

        do {
            let sync = try await sdk.getSyncStatusAsync(walletId: walletId)
            let currentBalance = try? await sdk.getBalanceAsync(walletId: walletId)
            let currentBirthday = try? await sdk.getLatestBirthdayHeightAsync(walletId: walletId)
            let currentTransactions = try? await sdk.listTransactionsAsync(
                walletId: walletId,
                limit: config.transactionLimit
            )

            let nextStatus: Status = sync.isComplete() ? .synced : .syncing

            publish(
                status: nextStatus,
                syncStatus: sync,
                latestBirthdayHeight: currentBirthday ?? latestBirthdayHeight,
                balance: currentBalance ?? balance,
                transactions: currentTransactions ?? transactions,
                lastError: nil,
                updatedAtMillis: observedAtMillis
            )

            if sync.isSyncing() {
                return config.syncingPollIntervalMs
            }
            return config.syncedPollIntervalMs
        } catch {
            publish(
                lastError: error,
                updatedAtMillis: observedAtMillis
            )
            return config.errorPollIntervalMs
        }
    }

    private func publish(
        status: Status? = nil,
        syncStatus: SyncStatus? = nil,
        latestBirthdayHeight: Int? = nil,
        balance: Balance? = nil,
        transactions: [TransactionInfo]? = nil,
        lastError: Error? = nil,
        updatedAtMillis: Int64? = nil
    ) {
        if let status { self.status = status }
        if let syncStatus { self.syncStatus = syncStatus }
        if let latestBirthdayHeight { self.latestBirthdayHeight = latestBirthdayHeight }
        if let balance { self.balance = balance }
        if let transactions { self.transactions = transactions }
        self.lastError = lastError

        let progressPercent = self.syncStatus?.percent ?? (self.status == .synced ? 100 : self.progress)
        self.progress = progressPercent

        snapshot = Snapshot(
            walletId: walletId,
            status: self.status,
            progressPercent: progressPercent,
            syncStatus: self.syncStatus,
            latestBirthdayHeight: self.latestBirthdayHeight,
            balance: self.balance,
            transactions: self.transactions,
            updatedAtMillis: updatedAtMillis ?? snapshot.updatedAtMillis,
            lastError: self.lastError
        )
    }

    private static func currentTimeMillis() -> Int64 {
        Int64(Date().timeIntervalSince1970 * 1000)
    }
}
