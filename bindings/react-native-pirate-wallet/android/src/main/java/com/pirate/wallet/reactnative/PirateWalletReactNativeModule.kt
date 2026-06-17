package com.pirate.wallet.reactnative

import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import java.io.File
import org.json.JSONObject

internal object NativeBridge {
    init {
        System.loadLibrary("pirate_ffi_native")
    }

    external fun invokeJson(requestJson: String, pretty: Boolean = false): String
}

class PirateWalletReactNativeModule(
    reactContext: ReactApplicationContext,
) : ReactContextBaseJavaModule(reactContext) {
    override fun getName(): String = "PirateWalletReactNative"

    @ReactMethod
    fun invoke(requestJson: String, pretty: Boolean, promise: Promise) {
        try {
            promise.resolve(NativeBridge.invokeJson(requestJson, pretty))
        } catch (t: Throwable) {
            promise.reject("PIRATE_WALLET_INVOKE_ERROR", t.message, t)
        }
    }

    @ReactMethod
    fun configureAccountStorage(
        accountId: String,
        passphrase: String,
        storagePath: String?,
        promise: Promise,
    ) {
        try {
            require(accountId.trim().isNotEmpty()) { "accountId must not be empty" }
            require(passphrase.isNotEmpty()) { "passphrase must not be empty" }

            val walletDbDir = accountStorageDirectory(reactApplicationContext, accountId, storagePath)
            ensureDirectory(walletDbDir)

            val requestJson = JSONObject()
                .put("method", "configure_wallet_storage")
                .put("base_dir", walletDbDir.absolutePath)
                .put("passphrase", passphrase)
                .toString()

            promise.resolve(NativeBridge.invokeJson(requestJson, false))
        } catch (t: Throwable) {
            promise.reject("PIRATE_WALLET_CONFIGURE_STORAGE_ERROR", t.message, t)
        }
    }

    private fun accountStorageDirectory(
        context: ReactApplicationContext,
        accountId: String,
        storagePath: String?,
    ): File {
        if (!storagePath.isNullOrBlank()) {
            return File(storagePath)
        }

        val accountsDir = File(File(context.filesDir, "pirate_wallet"), "accounts")
        return File(accountsDir, sanitizeAccountId(accountId))
    }

    private fun ensureDirectory(walletDbDir: File) {
        if (!walletDbDir.exists() && !walletDbDir.mkdirs()) {
            throw IllegalStateException(
                "Failed to create wallet database directory: ${walletDbDir.absolutePath}"
            )
        }
    }

    private fun sanitizeAccountId(accountId: String): String {
        val trimmed = accountId.trim()
        require(trimmed.isNotEmpty()) { "accountId must not be empty" }

        val sanitized = buildString {
            for (char in trimmed) {
                append(
                    if (char.isLetterOrDigit() || char == '_' || char == '-' || char == '.') {
                        char
                    } else {
                        '_'
                    }
                )
            }
        }
        return sanitized.ifEmpty { "account" }
    }
}
