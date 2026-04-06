package com.pirate.wallet.sdk

import java.io.Closeable
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

public class PirateWalletSynchronizer(
    private val sdk: PirateWalletSdk,
    public val walletId: String,
    public val config: Config = Config(),
    dispatcher: CoroutineDispatcher = Dispatchers.IO,
) : Closeable {
    public enum class Status {
        STOPPED,
        SYNCING,
        SYNCED,
    }

    public data class Config(
        val syncMode: SyncMode = SyncMode.Compact,
        val syncingPollIntervalMs: Long = DEFAULT_SYNCING_POLL_INTERVAL_MS,
        val syncedPollIntervalMs: Long = DEFAULT_SYNCED_POLL_INTERVAL_MS,
        val errorPollIntervalMs: Long = DEFAULT_ERROR_POLL_INTERVAL_MS,
        val transactionLimit: Int? = null,
    ) {
        init {
            require(syncingPollIntervalMs > 0) { "syncingPollIntervalMs must be greater than 0" }
            require(syncedPollIntervalMs > 0) { "syncedPollIntervalMs must be greater than 0" }
            require(errorPollIntervalMs > 0) { "errorPollIntervalMs must be greater than 0" }
            require(transactionLimit == null || transactionLimit > 0) {
                "transactionLimit must be greater than 0 when provided"
            }
        }
    }

    public data class Snapshot(
        val walletId: String,
        val status: Status,
        val progressPercent: Double,
        val syncStatus: SyncStatus?,
        val latestBirthdayHeight: Int?,
        val balance: Balance?,
        val transactions: List<TransactionInfo>,
        val updatedAtMillis: Long?,
        val lastError: Throwable?,
    ) {
        public fun isRunning(): Boolean = status != Status.STOPPED

        public fun isSyncing(): Boolean = status == Status.SYNCING

        public fun isComplete(): Boolean = syncStatus?.isComplete() == true
    }

    public companion object {
        public const val DEFAULT_SYNCING_POLL_INTERVAL_MS: Long = 1_000L
        public const val DEFAULT_SYNCED_POLL_INTERVAL_MS: Long = 5_000L
        public const val DEFAULT_ERROR_POLL_INTERVAL_MS: Long = 5_000L
    }

    private val closed = AtomicBoolean(false)
    private val lifecycleMutex = Mutex()
    private val refreshMutex = Mutex()
    private val scope = CoroutineScope(SupervisorJob() + dispatcher)

    @Volatile
    private var pollJob: Job? = null

    private val _status = MutableStateFlow(Status.STOPPED)
    private val _progress = MutableStateFlow(0.0)
    private val _syncStatus = MutableStateFlow<SyncStatus?>(null)
    private val _latestBirthdayHeight = MutableStateFlow<Int?>(null)
    private val _balance = MutableStateFlow<Balance?>(null)
    private val _transactions = MutableStateFlow<List<TransactionInfo>>(emptyList())
    private val _lastError = MutableStateFlow<Throwable?>(null)
    private val _snapshot = MutableStateFlow(
        Snapshot(
            walletId = walletId,
            status = Status.STOPPED,
            progressPercent = 0.0,
            syncStatus = null,
            latestBirthdayHeight = null,
            balance = null,
            transactions = emptyList(),
            updatedAtMillis = null,
            lastError = null,
        ),
    )

    public val status: StateFlow<Status> = _status.asStateFlow()
    public val progress: StateFlow<Double> = _progress.asStateFlow()
    public val syncStatus: StateFlow<SyncStatus?> = _syncStatus.asStateFlow()
    public val latestBirthdayHeight: Int?
        get() = _latestBirthdayHeight.value
    public val balance: StateFlow<Balance?> = _balance.asStateFlow()
    public val transactions: StateFlow<List<TransactionInfo>> = _transactions.asStateFlow()
    public val lastError: StateFlow<Throwable?> = _lastError.asStateFlow()
    public val snapshot: StateFlow<Snapshot> = _snapshot.asStateFlow()

    public fun currentSnapshot(): Snapshot = snapshot.value

    public fun isRunning(): Boolean = pollJob?.isActive == true

    public fun isSyncing(): Boolean = status.value == Status.SYNCING

    public fun isComplete(): Boolean = syncStatus.value?.isComplete() == true

    public fun start(): Job {
        check(!closed.get()) { "PirateWalletSynchronizer is closed" }

        return scope.launch {
            lifecycleMutex.withLock {
                check(!closed.get()) { "PirateWalletSynchronizer is closed" }
                if (pollJob?.isActive == true) {
                    return@withLock
                }

                publish(
                    status = Status.SYNCING,
                    lastError = null,
                    updatedAtMillis = System.currentTimeMillis(),
                )

                try {
                    sdk.startSync(walletId, config.syncMode)
                } catch (error: Throwable) {
                    publish(
                        status = Status.STOPPED,
                        lastError = error,
                        updatedAtMillis = System.currentTimeMillis(),
                    )
                    throw error
                }

                pollJob = scope.launch {
                    pollLoop()
                }
            }
        }
    }

    public fun stop(): Job = scope.launch {
        val stopState = lifecycleMutex.withLock {
            val existingJob = pollJob
            val shouldCancelBackend = existingJob != null || _status.value != Status.STOPPED
            pollJob = null
            publish(
                status = Status.STOPPED,
                updatedAtMillis = System.currentTimeMillis(),
            )
            existingJob to shouldCancelBackend
        }

        val existingJob = stopState.first
        val shouldCancelBackend = stopState.second

        existingJob?.cancelAndJoin()

        if (shouldCancelBackend) {
            try {
                sdk.cancelSync(walletId)
            } catch (error: Throwable) {
                publish(
                    status = Status.STOPPED,
                    lastError = error,
                    updatedAtMillis = System.currentTimeMillis(),
                )
                throw error
            }
        }
    }

    public fun refresh(): Job {
        check(!closed.get()) { "PirateWalletSynchronizer is closed" }

        return scope.launch {
            refreshOnce()
        }
    }

    public override fun close() {
        if (!closed.compareAndSet(false, true)) {
            return
        }

        runBlocking {
            stop().join()
        }

        scope.cancel()
    }

    private suspend fun pollLoop() {
        while (currentCoroutineContext().isActive) {
            val nextDelayMs = try {
                refreshOnce()
            } catch (_: Throwable) {
                config.errorPollIntervalMs
            }

            delay(nextDelayMs)
        }
    }

    private suspend fun refreshOnce(): Long = refreshMutex.withLock {
        val observedAtMillis = System.currentTimeMillis()

        val sync = try {
            sdk.getSyncStatus(walletId)
        } catch (error: Throwable) {
            publish(
                lastError = error,
                updatedAtMillis = observedAtMillis,
            )
            throw error
        }

        val balanceResult = runCatching {
            sdk.getBalance(walletId)
        }
        val latestBirthdayHeightResult = runCatching {
            sdk.getLatestBirthdayHeight(walletId)
        }
        val transactionsResult = runCatching {
            sdk.listTransactions(walletId, config.transactionLimit)
        }

        val partialError = balanceResult.exceptionOrNull()
            ?: latestBirthdayHeightResult.exceptionOrNull()
            ?: transactionsResult.exceptionOrNull()
        val nextStatus = if (pollJob?.isActive == true) {
            sync.toSynchronizerStatus()
        } else {
            _status.value
        }

        publish(
            status = nextStatus,
            syncStatus = sync,
            latestBirthdayHeight = latestBirthdayHeightResult.getOrElse { _latestBirthdayHeight.value },
            balance = balanceResult.getOrElse { _balance.value },
            transactions = transactionsResult.getOrElse { _transactions.value },
            lastError = partialError,
            updatedAtMillis = observedAtMillis,
        )

        if (partialError != null) {
            config.errorPollIntervalMs
        } else if (sync.isSyncing()) {
            config.syncingPollIntervalMs
        } else {
            config.syncedPollIntervalMs
        }
    }

    private fun publish(
        status: Status = _status.value,
        syncStatus: SyncStatus? = _syncStatus.value,
        latestBirthdayHeight: Int? = _latestBirthdayHeight.value,
        balance: Balance? = _balance.value,
        transactions: List<TransactionInfo> = _transactions.value,
        lastError: Throwable? = _lastError.value,
        updatedAtMillis: Long? = _snapshot.value.updatedAtMillis,
    ) {
        val progressPercent = syncStatus?.percent ?: when {
            status == Status.SYNCED -> 100.0
            else -> _progress.value
        }

        _status.value = status
        _syncStatus.value = syncStatus
        _latestBirthdayHeight.value = latestBirthdayHeight
        _balance.value = balance
        _transactions.value = transactions
        _lastError.value = lastError
        _progress.value = progressPercent
        _snapshot.value = Snapshot(
            walletId = walletId,
            status = status,
            progressPercent = progressPercent,
            syncStatus = syncStatus,
            latestBirthdayHeight = latestBirthdayHeight,
            balance = balance,
            transactions = transactions,
            updatedAtMillis = updatedAtMillis,
            lastError = lastError,
        )
    }
}

private fun SyncStatus.toSynchronizerStatus(): PirateWalletSynchronizer.Status = when {
    isComplete() -> PirateWalletSynchronizer.Status.SYNCED
    else -> PirateWalletSynchronizer.Status.SYNCING
}
