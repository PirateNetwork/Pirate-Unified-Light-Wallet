package com.pirate.wallet

import android.content.Context
import android.system.Os
import java.io.File

/** Configures writable, app-private locations used by native wallet code. */
internal object NativeRuntimeEnvironment {
    fun configure(context: Context) {
        val filesDir = context.filesDir
        val cacheDir = context.cacheDir
        val walletDir = File(filesDir, "wallets")
        val torStateDir = File(filesDir, "tor/state")
        val torCacheDir = File(cacheDir, "pirate_wallet/tor")
        val debugLogDir = File(filesDir, "logs")

        listOf(walletDir, torStateDir, torCacheDir, debugLogDir).forEach { directory ->
            runCatching { directory.mkdirs() }
        }

        // Android 12 sets java.io.tmpdir but does not export TMPDIR for native
        // code. Keep both runtimes on the app-private cache directory.
        System.setProperty("java.io.tmpdir", cacheDir.absolutePath)

        mapOf(
            "TMPDIR" to cacheDir.absolutePath,
            "PIRATE_WALLET_DB_DIR" to walletDir.absolutePath,
            "PIRATE_TOR_STATE_DIR" to torStateDir.absolutePath,
            "PIRATE_TOR_CACHE_DIR" to torCacheDir.absolutePath,
            "PIRATE_DEBUG_LOG_PATH" to File(debugLogDir, "debug.log").absolutePath,
        ).forEach { (name, value) ->
            runCatching { Os.setenv(name, value, true) }
        }
    }
}
