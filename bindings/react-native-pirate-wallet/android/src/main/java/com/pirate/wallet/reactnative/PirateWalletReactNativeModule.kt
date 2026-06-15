package com.pirate.wallet.reactnative

import android.system.ErrnoException
import android.system.Os
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import java.io.File

internal object NativeBridge {
    init {
        System.loadLibrary("pirate_ffi_native")
    }

    external fun invokeJson(requestJson: String, pretty: Boolean = false): String
}

class PirateWalletReactNativeModule(
    reactContext: ReactApplicationContext,
) : ReactContextBaseJavaModule(reactContext) {
    init {
        configureWalletDatabaseDirectory(reactContext)
    }

    override fun getName(): String = "PirateWalletReactNative"

    @ReactMethod
    fun invoke(requestJson: String, pretty: Boolean, promise: Promise) {
        try {
            promise.resolve(NativeBridge.invokeJson(requestJson, pretty))
        } catch (t: Throwable) {
            promise.reject("PIRATE_WALLET_INVOKE_ERROR", t.message, t)
        }
    }

    private fun configureWalletDatabaseDirectory(context: ReactApplicationContext) {
        val walletDbDir = File(context.filesDir, "pirate_wallet")

        if (!walletDbDir.exists() && !walletDbDir.mkdirs()) {
            throw IllegalStateException(
                "Failed to create wallet database directory: ${walletDbDir.absolutePath}"
            )
        }

        try {
            Os.setenv("PIRATE_WALLET_DB_DIR", walletDbDir.absolutePath, true)
        } catch (e: ErrnoException) {
            throw IllegalStateException("Failed to configure PIRATE_WALLET_DB_DIR", e)
        }
    }
}
