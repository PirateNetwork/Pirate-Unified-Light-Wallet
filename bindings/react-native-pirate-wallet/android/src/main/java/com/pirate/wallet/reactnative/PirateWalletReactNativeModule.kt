package com.pirate.wallet.reactnative

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import java.io.File
import java.security.MessageDigest
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
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

    @ReactMethod
    fun configureSecureAccountStorage(
        accountId: String,
        storagePath: String?,
        promise: Promise,
    ) {
        try {
            require(accountId.trim().isNotEmpty()) { "accountId must not be empty" }
            val passphrase = secureRegistryPassphrase(accountId)
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

    private fun secureRegistryPassphrase(accountId: String): String {
        val accountKey = MessageDigest.getInstance("SHA-256")
            .digest(accountId.trim().toByteArray(Charsets.UTF_8))
            .joinToString("") { byte -> "%02x".format(byte.toInt() and 0xff) }
        val alias = "pirate_wallet_registry_$accountKey"
        val preferences = reactApplicationContext.getSharedPreferences(
            "pirate_wallet_registry_credentials",
            Context.MODE_PRIVATE,
        )
        val stored = preferences.getString(accountKey, null)
        val key = getOrCreateKeystoreKey(alias)
        if (stored != null) {
            val envelope = Base64.decode(stored, Base64.NO_WRAP)
            require(envelope.size > 12) { "Stored registry credential is invalid" }
            val iv = envelope.copyOfRange(0, 12)
            val ciphertext = envelope.copyOfRange(12, envelope.size)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, iv))
            return String(cipher.doFinal(ciphertext), Charsets.UTF_8)
        }

        val random = ByteArray(32)
        SecureRandom().nextBytes(random)
        val passphrase = Base64.encodeToString(random, Base64.NO_WRAP)
        random.fill(0)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key)
        val ciphertext = cipher.doFinal(passphrase.toByteArray(Charsets.UTF_8))
        val envelope = cipher.iv + ciphertext
        check(
            preferences.edit()
                .putString(accountKey, Base64.encodeToString(envelope, Base64.NO_WRAP))
                .commit()
        ) { "Failed to persist protected registry credential" }
        return passphrase
    }

    private fun getOrCreateKeystoreKey(alias: String): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (keyStore.getKey(alias, null) as? SecretKey)?.let { return it }

        val specBuilder = KeyGenParameterSpec.Builder(
            alias,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setRandomizedEncryptionRequired(true)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            specBuilder.setUnlockedDeviceRequired(true)
        }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
            .apply { init(specBuilder.build()) }
            .generateKey()
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
