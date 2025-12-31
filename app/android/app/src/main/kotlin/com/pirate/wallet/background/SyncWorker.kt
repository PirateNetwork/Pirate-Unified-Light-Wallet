package com.pirate.wallet.background

import android.content.Context
import android.content.SharedPreferences
import android.content.pm.ServiceInfo
import android.os.Build
import androidx.work.*
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.dart.DartExecutor
import io.flutter.plugin.common.MethodChannel
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/**
 * Background sync worker for Android
 * 
 * Handles periodic blockchain synchronization using WorkManager with:
 * - SyncCompact: Every 15 minutes with battery/network constraints
 * - SyncDeep: Daily when on charger and unmetered network (WiFi)
 * - Foreground service for long-running operations
 * - Privacy-respecting network tunnel (Tor/SOCKS5)
 * - All RPC calls routed through configured NetTunnel
 * 
 * Acceptance criteria:
 * - Balances update without foregrounding the app
 * - Disabling Tor causes clean failure notification
 */
class SyncWorker(
    context: Context,
    params: WorkerParameters
) : CoroutineWorker(context, params) {

    companion object {
        const val WORK_NAME_COMPACT = "pirate_sync_compact"
        const val WORK_NAME_DEEP = "pirate_sync_deep"
        
        const val CHANNEL_ID_SYNC = NotificationChannels.CHANNEL_SYNC
        const val CHANNEL_ID_TX = NotificationChannels.CHANNEL_TRANSACTIONS
        
        const val NOTIFICATION_ID_SYNC = 1001
        const val NOTIFICATION_ID_TX = 1002
        const val NOTIFICATION_ID_NETWORK_ERROR = 1003
        
        private const val TAG = "PirateSyncWorker"
        private const val PREFS_NAME = "pirate_sync_prefs"
        private const val KEY_LAST_COMPACT_SYNC = "last_compact_sync"
        private const val KEY_LAST_DEEP_SYNC = "last_deep_sync"
        private const val KEY_TUNNEL_MODE = "tunnel_mode"
        private const val KEY_SOCKS5_URL = "socks5_url"
        private const val KEY_ACTIVE_WALLET_ID = "active_wallet_id"
        
        // Sync intervals
        const val COMPACT_INTERVAL_MINUTES = 15L
        const val COMPACT_FLEX_MINUTES = 5L
        const val DEEP_INTERVAL_HOURS = 24L
        const val DEEP_FLEX_HOURS = 2L
        
        // Tunnel modes (must match Rust TunnelMode enum)
        const val TUNNEL_TOR = "tor"
        const val TUNNEL_SOCKS5 = "socks5"
        const val TUNNEL_DIRECT = "direct"
        
        // Method channel for FFI communication
        private const val CHANNEL_NAME = "com.pirate.wallet/background_sync"

        /**
         * Schedule periodic compact sync (every 15 minutes)
         * Uses battery and network constraints for efficiency
         */
        fun scheduleCompactSync(context: Context) {
            val constraints = Constraints.Builder()
                .setRequiredNetworkType(NetworkType.CONNECTED)
                .setRequiresBatteryNotLow(true)
                .build()

            val syncRequest = PeriodicWorkRequestBuilder<SyncWorker>(
                COMPACT_INTERVAL_MINUTES, TimeUnit.MINUTES,
                COMPACT_FLEX_MINUTES, TimeUnit.MINUTES
            )
                .setConstraints(constraints)
                .setInputData(workDataOf(
                    "sync_mode" to "compact",
                    "max_duration_secs" to 60L,
                    "max_blocks" to 5000L
                ))
                .setBackoffCriteria(
                    BackoffPolicy.EXPONENTIAL,
                    WorkRequest.MIN_BACKOFF_MILLIS,
                    TimeUnit.MILLISECONDS
                )
                .addTag("pirate_sync")
                .addTag("compact")
                .build()

            WorkManager.getInstance(context).enqueueUniquePeriodicWork(
                WORK_NAME_COMPACT,
                ExistingPeriodicWorkPolicy.UPDATE,
                syncRequest
            )
            
            android.util.Log.i(TAG, "Scheduled compact sync: every ${COMPACT_INTERVAL_MINUTES}m (flex ${COMPACT_FLEX_MINUTES}m)")
        }

        /**
         * Schedule periodic deep sync (daily)
         * Requires WiFi (unmetered) and charging for battery efficiency
         */
        fun scheduleDeepSync(context: Context) {
            val constraints = Constraints.Builder()
                .setRequiredNetworkType(NetworkType.UNMETERED) // WiFi only
                .setRequiresCharging(true)
                .setRequiresBatteryNotLow(true)
                .build()

            val syncRequest = PeriodicWorkRequestBuilder<SyncWorker>(
                DEEP_INTERVAL_HOURS, TimeUnit.HOURS,
                DEEP_FLEX_HOURS, TimeUnit.HOURS
            )
                .setConstraints(constraints)
                .setInputData(workDataOf(
                    "sync_mode" to "deep",
                    "max_duration_secs" to 300L,  // 5 minutes
                    "max_blocks" to 50000L
                ))
                .setBackoffCriteria(
                    BackoffPolicy.EXPONENTIAL,
                    WorkRequest.MIN_BACKOFF_MILLIS,
                    TimeUnit.MILLISECONDS
                )
                .addTag("pirate_sync")
                .addTag("deep")
                .build()

            WorkManager.getInstance(context).enqueueUniquePeriodicWork(
                WORK_NAME_DEEP,
                ExistingPeriodicWorkPolicy.UPDATE,
                syncRequest
            )
            
            android.util.Log.i(TAG, "Scheduled deep sync: every ${DEEP_INTERVAL_HOURS}h (flex ${DEEP_FLEX_HOURS}h), WiFi + charging")
        }

        /**
         * Schedule both sync types
         */
        fun scheduleAllSyncs(context: Context) {
            scheduleCompactSync(context)
            scheduleDeepSync(context)
            android.util.Log.i(TAG, "Scheduled all background syncs")
        }

        /**
         * Trigger immediate sync (for user-initiated refresh)
         */
        fun triggerImmediateSync(context: Context, mode: String = "compact") {
            val constraints = Constraints.Builder()
                .setRequiredNetworkType(NetworkType.CONNECTED)
                .build()

            val syncRequest = OneTimeWorkRequestBuilder<SyncWorker>()
                .setConstraints(constraints)
                .setInputData(workDataOf(
                    "sync_mode" to mode,
                    "max_duration_secs" to if (mode == "deep") 300L else 120L,
                    "immediate" to true
                ))
                .setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
                .addTag("pirate_sync")
                .addTag("immediate")
                .build()

            WorkManager.getInstance(context).enqueue(syncRequest)
            android.util.Log.i(TAG, "Triggered immediate $mode sync")
        }

        /**
         * Cancel all scheduled sync work
         */
        fun cancelAllSync(context: Context) {
            WorkManager.getInstance(context).cancelAllWorkByTag("pirate_sync")
            android.util.Log.i(TAG, "Cancelled all scheduled sync work")
        }

        /**
         * Cancel only background syncs (keep immediate)
         */
        fun cancelBackgroundSync(context: Context) {
            WorkManager.getInstance(context).cancelUniqueWork(WORK_NAME_COMPACT)
            WorkManager.getInstance(context).cancelUniqueWork(WORK_NAME_DEEP)
            android.util.Log.i(TAG, "Cancelled background sync work")
        }

        /**
         * Configure network tunnel mode
         */
        fun setTunnelMode(context: Context, mode: String, socks5Url: String? = null) {
            val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            prefs.edit().apply {
                putString(KEY_TUNNEL_MODE, mode)
                if (mode == TUNNEL_SOCKS5 && socks5Url != null) {
                    putString(KEY_SOCKS5_URL, socks5Url)
                }
                apply()
            }
            android.util.Log.i(TAG, "Set tunnel mode: $mode")
        }

        /**
         * Set active wallet ID for background sync
         */
        fun setActiveWalletId(context: Context, walletId: String) {
            val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            prefs.edit().putString(KEY_ACTIVE_WALLET_ID, walletId).apply()
            android.util.Log.i(TAG, "Set active wallet ID: $walletId")
        }

        /**
         * Get sync status for UI
         */
        fun getSyncStatus(context: Context): SyncStatus {
            val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            return SyncStatus(
                lastCompactSync = prefs.getLong(KEY_LAST_COMPACT_SYNC, 0),
                lastDeepSync = prefs.getLong(KEY_LAST_DEEP_SYNC, 0),
                tunnelMode = prefs.getString(KEY_TUNNEL_MODE, TUNNEL_TOR) ?: TUNNEL_TOR
            )
        }
    }

    private val prefs: SharedPreferences by lazy {
        applicationContext.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
    }

    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val syncMode = inputData.getString("sync_mode") ?: "compact"
        val maxDurationSecs = inputData.getLong("max_duration_secs", 60L)
        val maxBlocks = inputData.getLong("max_blocks", 5000L)
        val isImmediate = inputData.getBoolean("immediate", false)
        
        android.util.Log.i(TAG, "Starting sync: mode=$syncMode, attempt=${runAttemptCount}, immediate=$isImmediate")

        // Prefer active wallet if available; otherwise fall back to round-robin selection.
        val walletId = prefs.getString(KEY_ACTIVE_WALLET_ID, null)
        val useRoundRobin = walletId.isNullOrEmpty()
        if (useRoundRobin) {
            android.util.Log.i(TAG, "No active wallet ID, using round-robin selection")
        }

        try {
            // Ensure notification channel exists
            NotificationChannels.createChannels(applicationContext)
            
            // Show foreground notification for long operations or deep sync
            if (syncMode == "deep" || maxDurationSecs > 60 || runAttemptCount > 0) {
                setForeground(createForegroundInfo(syncMode))
            }

            // Get tunnel configuration
            val tunnelConfig = getTunnelConfig()
            android.util.Log.d(TAG, "Using tunnel: ${tunnelConfig.mode}")

            // Execute sync through FFI bridge (all RPC via NetTunnel)
            val result = executeSync(
                walletId,
                syncMode,
                maxDurationSecs,
                maxBlocks,
                tunnelConfig,
                useRoundRobin
            )
            
            android.util.Log.i(
                TAG, 
                "Sync completed: blocks=${result.blocksSynced}, duration=${result.durationSecs}s, new_txs=${result.newTransactions}"
            )

            // Update last sync timestamps
            updateLastSyncTime(syncMode)

            // Show notification if new transactions received
            if (result.newTransactions > 0) {
                showNewTransactionNotification(result.newTransactions, result.newBalance ?: 0)
            }

            // Update widget if available
            updateWidget(result)

            Result.success(
                workDataOf(
                    "blocks_synced" to result.blocksSynced,
                    "new_transactions" to result.newTransactions,
                    "duration_secs" to result.durationSecs,
                    "tunnel_used" to result.tunnelUsed
                )
            )
        } catch (e: TorConnectionException) {
            android.util.Log.e(TAG, "Tor connection failed: ${e.message}", e)
            
            // Show clean notification about Tor failure
            showTorFailureNotification(e.message ?: "Tor connection failed")
            
            // Retry with backoff (Tor may come back)
            if (runAttemptCount < 3) {
                Result.retry()
            } else {
                Result.failure(workDataOf(
                    "error" to "tor_connection_failed",
                    "message" to (e.message ?: "Tor connection failed after multiple attempts")
                ))
            }
        } catch (e: Socks5ConnectionException) {
            android.util.Log.e(TAG, "SOCKS5 proxy failed: ${e.message}", e)
            
            showNetworkErrorNotification("SOCKS5 proxy connection failed: ${e.message}")
            
            if (runAttemptCount < 2) {
                Result.retry()
            } else {
                Result.failure(workDataOf("error" to "socks5_connection_failed"))
            }
        } catch (e: NetworkTunnelException) {
            android.util.Log.e(TAG, "Network tunnel error: ${e.message}", e)
            
            // Show notification about network issue
            showNetworkErrorNotification(e.message ?: "Connection failed")
            
            // Retry with backoff
            if (runAttemptCount < 3) {
                Result.retry()
            } else {
                Result.failure(workDataOf("error" to "network_tunnel_failed"))
            }
        } catch (e: Exception) {
            android.util.Log.e(TAG, "Sync failed: ${e.message}", e)
            
            // Retry with exponential backoff
            if (runAttemptCount < 3) {
                Result.retry()
            } else {
                Result.failure(workDataOf("error" to (e.message ?: "Unknown error")))
            }
        }
    }

    /**
     * Get tunnel configuration from preferences
     */
    private fun getTunnelConfig(): TunnelConfig {
        val mode = prefs.getString(KEY_TUNNEL_MODE, TUNNEL_TOR) ?: TUNNEL_TOR
        val socks5Url = prefs.getString(KEY_SOCKS5_URL, null)
        
        return TunnelConfig(
            mode = mode,
            socks5Url = if (mode == TUNNEL_SOCKS5) socks5Url else null
        )
    }

    /**
     * Update last sync timestamp
     */
    private fun updateLastSyncTime(mode: String) {
        val key = if (mode == "deep") KEY_LAST_DEEP_SYNC else KEY_LAST_COMPACT_SYNC
        prefs.edit().putLong(key, System.currentTimeMillis()).apply()
    }

    /**
     * Create foreground service notification
     */
    private fun createForegroundInfo(syncMode: String): ForegroundInfo {
        val tunnelMode = prefs.getString(KEY_TUNNEL_MODE, TUNNEL_TOR) ?: TUNNEL_TOR
        val tunnelText = when (tunnelMode) {
            TUNNEL_TOR -> "via Tor 🧅"
            TUNNEL_SOCKS5 -> "via SOCKS5"
            else -> "directly"
        }

        val title = if (syncMode == "deep") {
            "Deep Sync in Progress"
        } else {
            "Syncing Blockchain"
        }

        val notification = NotificationCompat.Builder(applicationContext, CHANNEL_ID_SYNC)
            .setContentTitle(title)
            .setContentText("Updating your balance securely $tunnelText...")
            .setSmallIcon(android.R.drawable.ic_popup_sync)
            .setOngoing(true)
            .setSilent(true)
            .setProgress(0, 0, true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            .build()

        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            ForegroundInfo(
                NOTIFICATION_ID_SYNC,
                notification,
                ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
            )
        } else {
            ForegroundInfo(NOTIFICATION_ID_SYNC, notification)
        }
    }

    /**
     * Execute sync via FFI bridge with tunnel configuration
     * All RPC calls are routed through the configured NetTunnel (Tor/SOCKS5/Direct)
     */
    private suspend fun executeSync(
        walletId: String?,
        mode: String,
        maxDurationSecs: Long,
        maxBlocks: Long,
        tunnelConfig: TunnelConfig,
        useRoundRobin: Boolean
    ): SyncResult {
        val startTime = System.currentTimeMillis()
        
        return suspendCancellableCoroutine { continuation ->
            try {
                // Create headless Flutter engine for FFI calls
                val flutterEngine = FlutterEngine(applicationContext)
                flutterEngine.dartExecutor.executeDartEntrypoint(
                    DartExecutor.DartEntrypoint.createDefault()
                )
                
                val channel = MethodChannel(
                    flutterEngine.dartExecutor.binaryMessenger,
                    CHANNEL_NAME
                )
                
                // Call FFI through method channel
                channel.invokeMethod(
                    "executeBackgroundSync",
                    mapOf(
                        "walletId" to walletId,
                        "mode" to mode,
                        "maxDurationSecs" to maxDurationSecs,
                        "maxBlocks" to maxBlocks,
                        "useRoundRobin" to useRoundRobin,
                        "tunnelMode" to tunnelConfig.mode,
                        "socks5Url" to tunnelConfig.socks5Url
                    ),
                    object : MethodChannel.Result {
                        override fun success(result: Any?) {
                            @Suppress("UNCHECKED_CAST")
                            val resultMap = result as? Map<String, Any?> ?: emptyMap()
                            
                            val duration = ((System.currentTimeMillis() - startTime) / 1000).toInt()
                            
                            // Check for tunnel errors
                            val errors = resultMap["errors"] as? List<String> ?: emptyList()
                            if (errors.any { it.contains("tor", ignoreCase = true) }) {
                                flutterEngine.destroy()
                                continuation.resumeWithException(
                                    TorConnectionException(errors.firstOrNull() ?: "Tor connection failed")
                                )
                                return
                            }
                            if (errors.any { it.contains("socks5", ignoreCase = true) }) {
                                flutterEngine.destroy()
                                continuation.resumeWithException(
                                    Socks5ConnectionException(errors.firstOrNull() ?: "SOCKS5 connection failed")
                                )
                                return
                            }
                            
                            val syncResult = SyncResult(
                                mode = mode,
                                blocksSynced = (resultMap["blocks_synced"] as? Number)?.toLong() ?: 0,
                                durationSecs = duration.toLong(),
                                newTransactions = (resultMap["new_transactions"] as? Number)?.toInt() ?: 0,
                                newBalance = (resultMap["new_balance"] as? Number)?.toLong(),
                                tunnelUsed = resultMap["tunnel_used"] as? String ?: tunnelConfig.mode
                            )
                            
                            flutterEngine.destroy()
                            continuation.resume(syncResult)
                        }
                        
                        override fun error(errorCode: String, errorMessage: String?, errorDetails: Any?) {
                            flutterEngine.destroy()
                            
                            // Map error codes to specific exceptions
                            when (errorCode) {
                                "TOR_CONNECTION_FAILED" -> 
                                    continuation.resumeWithException(
                                        TorConnectionException(errorMessage ?: "Tor connection failed")
                                    )
                                "SOCKS5_CONNECTION_FAILED" ->
                                    continuation.resumeWithException(
                                        Socks5ConnectionException(errorMessage ?: "SOCKS5 connection failed")
                                    )
                                "NETWORK_ERROR" ->
                                    continuation.resumeWithException(
                                        NetworkTunnelException(errorMessage ?: "Network error")
                                    )
                                else ->
                                    continuation.resumeWithException(
                                        Exception("Sync failed: $errorCode - $errorMessage")
                                    )
                            }
                        }
                        
                        override fun notImplemented() {
                            flutterEngine.destroy()
                            continuation.resumeWithException(
                                Exception("Background sync not implemented in Flutter")
                            )
                        }
                    }
                )
                
                continuation.invokeOnCancellation {
                    flutterEngine.destroy()
                }
            } catch (e: Exception) {
                android.util.Log.e(TAG, "Failed to execute FFI sync: ${e.message}", e)
                continuation.resumeWithException(e)
            }
        }
    }

    /**
     * Show notification for new transactions
     */
    private fun showNewTransactionNotification(count: Int, balance: Long) {
        // Check notification permission (Android 13+)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (!NotificationManagerCompat.from(applicationContext).areNotificationsEnabled()) {
                android.util.Log.w(TAG, "Notifications disabled, skipping tx notification")
                return
            }
        }

        val notification = NotificationCompat.Builder(applicationContext, CHANNEL_ID_TX)
            .setContentTitle("Received $count Payment${if (count > 1) "s" else ""}")
            .setContentText("New balance: ${formatBalance(balance)} ARRR")
            .setSmallIcon(android.R.drawable.ic_dialog_email)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setAutoCancel(true)
            .setNumber(count)
            .build()

        try {
            NotificationManagerCompat.from(applicationContext).notify(NOTIFICATION_ID_TX, notification)
        } catch (e: SecurityException) {
            android.util.Log.w(TAG, "Missing notification permission: ${e.message}")
        }
    }

    /**
     * Show notification for Tor connection failure
     * This provides a clean user-facing message when Tor is disabled or fails
     */
    private fun showTorFailureNotification(message: String) {
        val notification = NotificationCompat.Builder(applicationContext, CHANNEL_ID_SYNC)
            .setContentTitle("Privacy Connection Failed")
            .setContentText("Unable to connect via Tor. Tap to configure network settings.")
            .setStyle(NotificationCompat.BigTextStyle()
                .bigText("$message\n\nYour wallet requires a privacy-preserving connection. " +
                        "Please check your Tor settings or configure a SOCKS5 proxy."))
            .setSmallIcon(android.R.drawable.ic_dialog_alert)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .setAutoCancel(true)
            .setCategory(NotificationCompat.CATEGORY_ERROR)
            .build()

        try {
            NotificationManagerCompat.from(applicationContext).notify(NOTIFICATION_ID_NETWORK_ERROR, notification)
        } catch (e: SecurityException) {
            android.util.Log.w(TAG, "Missing notification permission: ${e.message}")
        }
    }

    /**
     * Show notification for network errors
     */
    private fun showNetworkErrorNotification(message: String) {
        val notification = NotificationCompat.Builder(applicationContext, CHANNEL_ID_SYNC)
            .setContentTitle("Sync Connection Issue")
            .setContentText(message)
            .setSmallIcon(android.R.drawable.ic_dialog_alert)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setAutoCancel(true)
            .build()

        try {
            NotificationManagerCompat.from(applicationContext).notify(NOTIFICATION_ID_NETWORK_ERROR, notification)
        } catch (e: SecurityException) {
            android.util.Log.w(TAG, "Missing notification permission: ${e.message}")
        }
    }

    /**
     * Update home screen widget
     */
    private fun updateWidget(result: SyncResult) {
        // Widget update logic would go here
        android.util.Log.d(TAG, "Would update widget: blocks=${result.blocksSynced}, balance=${result.newBalance}")
    }

    /**
     * Format balance for display
     */
    private fun formatBalance(zatoshis: Long): String {
        val arrr = zatoshis.toDouble() / 100_000_000
        return "%.4f".format(arrr)
    }
}

/**
 * Tunnel configuration
 */
data class TunnelConfig(
    val mode: String,
    val socks5Url: String? = null
)

/**
 * Sync result data class
 */
data class SyncResult(
    val mode: String,
    val blocksSynced: Long,
    val durationSecs: Long,
    val newTransactions: Int,
    val newBalance: Long?,
    val tunnelUsed: String
)

/**
 * Sync status for UI
 */
data class SyncStatus(
    val lastCompactSync: Long,
    val lastDeepSync: Long,
    val tunnelMode: String
) {
    val minutesSinceCompact: Long
        get() = if (lastCompactSync > 0) {
            (System.currentTimeMillis() - lastCompactSync) / 60000
        } else -1

    val hoursSinceDeep: Long
        get() = if (lastDeepSync > 0) {
            (System.currentTimeMillis() - lastDeepSync) / 3600000
        } else -1
}

/**
 * Base network tunnel exception
 */
open class NetworkTunnelException(message: String, cause: Throwable? = null) : Exception(message, cause)

/**
 * Tor-specific connection exception
 * Thrown when Tor connection fails or is disabled
 */
class TorConnectionException(message: String, cause: Throwable? = null) : NetworkTunnelException(message, cause)

/**
 * SOCKS5-specific connection exception
 */
class Socks5ConnectionException(message: String, cause: Throwable? = null) : NetworkTunnelException(message, cause)
