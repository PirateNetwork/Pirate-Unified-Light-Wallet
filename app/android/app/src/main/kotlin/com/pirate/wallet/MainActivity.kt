package com.pirate.wallet

import io.flutter.embedding.android.FlutterFragmentActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import android.os.Bundle
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.system.Os
import android.view.WindowManager
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import java.security.KeyStore
import java.io.File
import java.util.concurrent.Executor
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import com.pirate.wallet.background.SyncWorker
import com.pirate.wallet.background.NotificationChannels

class MainActivity: FlutterFragmentActivity() {
    
    private val CHANNEL = "com.pirate.wallet/background"
    private val KEYSTORE_CHANNEL = "com.pirate.wallet/keystore"
    private val SECURITY_CHANNEL = "com.pirate.wallet/security"
    private val PREFS_NAME = "pirate_keystore_v1"
    private val PREFS_BIOMETRICS_ENABLED = "biometrics_enabled_v1"
    private val STORE_KEY_ALIAS = "pirate_wallet_store_key_v1"
    private val MASTER_KEY_ALIAS = "pirate_wallet_master_key_v1"
    
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        
        // Create notification channels
        NotificationChannels.createChannels(this)
        
        // Set up method channel for background sync
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, CHANNEL).setMethodCallHandler { call, result ->
            when (call.method) {
                "initializeBackgroundSync" -> {
                    // Schedule WorkManager jobs
                    SyncWorker.scheduleCompactSync(this)
                    SyncWorker.scheduleDeepSync(this)
                    result.success(true)
                }
                "cancelBackgroundSync" -> {
                    SyncWorker.cancelAllSync(this)
                    result.success(true)
                }
                else -> {
                    result.notImplemented()
                }
            }
        }

        // Set up method channel for keystore operations
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, KEYSTORE_CHANNEL).setMethodCallHandler { call, result ->
            try {
                when (call.method) {
                    "storeKey" -> {
                        val keyId = call.argument<String>("keyId")
                        val encryptedKey = call.argument<ByteArray>("encryptedKey")
                        if (keyId == null || encryptedKey == null) {
                            result.error("INVALID_ARGUMENT", "keyId and encryptedKey required", null)
                            return@setMethodCallHandler
                        }
                        storeKey(keyId, encryptedKey)
                        result.success(true)
                    }
                    "retrieveKey" -> {
                        val keyId = call.argument<String>("keyId")
                        if (keyId == null) {
                            result.error("INVALID_ARGUMENT", "keyId required", null)
                            return@setMethodCallHandler
                        }
                        val data = retrieveKey(keyId)
                        result.success(data)
                    }
                    "deleteKey" -> {
                        val keyId = call.argument<String>("keyId")
                        if (keyId == null) {
                            result.error("INVALID_ARGUMENT", "keyId required", null)
                            return@setMethodCallHandler
                        }
                        deleteKey(keyId)
                        result.success(true)
                    }
                    "keyExists" -> {
                        val keyId = call.argument<String>("keyId")
                        if (keyId == null) {
                            result.error("INVALID_ARGUMENT", "keyId required", null)
                            return@setMethodCallHandler
                        }
                        result.success(keyExists(keyId))
                    }
                    "sealMasterKey" -> {
                        val masterKey = call.argument<ByteArray>("masterKey")
                        if (masterKey == null) {
                            result.error("INVALID_ARGUMENT", "masterKey required", null)
                            return@setMethodCallHandler
                        }
                        if (requiresBiometric(MASTER_KEY_ALIAS)) {
                            encryptWithBiometric(MASTER_KEY_ALIAS, masterKey, result)
                        } else {
                            val sealed = encryptWithAlias(MASTER_KEY_ALIAS, masterKey)
                            result.success(sealed)
                        }
                    }
                    "unsealMasterKey" -> {
                        val sealedKey = call.argument<ByteArray>("sealedKey")
                        if (sealedKey == null) {
                            result.error("INVALID_ARGUMENT", "sealedKey required", null)
                            return@setMethodCallHandler
                        }
                        if (requiresBiometric(MASTER_KEY_ALIAS)) {
                            decryptWithBiometric(MASTER_KEY_ALIAS, sealedKey, result)
                        } else {
                            val unsealed = decryptWithAlias(MASTER_KEY_ALIAS, sealedKey)
                            result.success(unsealed)
                        }
                    }
                    "getCapabilities" -> {
                        result.success(getCapabilities())
                    }
                    "setBiometricsEnabled" -> {
                        val enabled = call.argument<Boolean>("enabled")
                        if (enabled == null) {
                            result.error("INVALID_ARGUMENT", "enabled required", null)
                            return@setMethodCallHandler
                        }
                        setBiometricsEnabled(enabled)
                        result.success(true)
                    }
                    else -> result.notImplemented()
                }
            } catch (e: Exception) {
                result.error("KEYSTORE_ERROR", e.message, null)
            }
        }

        // Set up method channel for screenshot protection
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, SECURITY_CHANNEL).setMethodCallHandler { call, result ->
            when (call.method) {
                "enableScreenshotProtection" -> {
                    runOnUiThread {
                        window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
                    }
                    result.success(true)
                }
                "disableScreenshotProtection" -> {
                    runOnUiThread {
                        window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
                    }
                    result.success(true)
                }
                else -> result.notImplemented()
            }
        }
    }
    
    override fun onCreate(savedInstanceState: Bundle?) {
        try {
            val walletDir = File(filesDir, "wallets")
            if (!walletDir.exists()) {
                walletDir.mkdirs()
            }
            Os.setenv("PIRATE_WALLET_DB_DIR", walletDir.absolutePath, true)
        } catch (_: Exception) {
            // Best-effort; Rust will fall back if env cannot be set.
        }
        super.onCreate(savedInstanceState)
        
        // Initialize background sync on app start
        SyncWorker.scheduleCompactSync(this)
        SyncWorker.scheduleDeepSync(this)
    }

    private fun storeKey(keyId: String, data: ByteArray) {
        val encrypted = encryptWithAlias(STORE_KEY_ALIAS, data)
        val prefs = getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
        prefs.edit()
            .putString(keyId, Base64.encodeToString(encrypted, Base64.NO_WRAP))
            .apply()
    }

    private fun retrieveKey(keyId: String): ByteArray? {
        val prefs = getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
        val encoded = prefs.getString(keyId, null) ?: return null
        val encrypted = Base64.decode(encoded, Base64.NO_WRAP)
        return decryptWithAlias(STORE_KEY_ALIAS, encrypted)
    }

    private fun deleteKey(keyId: String) {
        val prefs = getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
        prefs.edit().remove(keyId).apply()
    }

    private fun keyExists(keyId: String): Boolean {
        val prefs = getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
        return prefs.contains(keyId)
    }

    private fun encryptWithAlias(alias: String, plaintext: ByteArray): ByteArray {
        val secretKey = getOrCreateSecretKey(alias)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, secretKey)
        val iv = cipher.iv
        val ciphertext = cipher.doFinal(plaintext)
        val out = ByteArray(iv.size + ciphertext.size)
        System.arraycopy(iv, 0, out, 0, iv.size)
        System.arraycopy(ciphertext, 0, out, iv.size, ciphertext.size)
        return out
    }

    private fun decryptWithAlias(alias: String, sealed: ByteArray): ByteArray {
        if (sealed.size < 13) {
            throw IllegalArgumentException("sealed data too short")
        }
        val secretKey = getOrCreateSecretKey(alias)
        val iv = sealed.copyOfRange(0, 12)
        val ciphertext = sealed.copyOfRange(12, sealed.size)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, secretKey, GCMParameterSpec(128, iv))
        return cipher.doFinal(ciphertext)
    }

    private fun getOrCreateSecretKey(alias: String): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore")
        keyStore.load(null)
        val existingKey = keyStore.getKey(alias, null) as? SecretKey
        if (existingKey != null) {
            return existingKey
        }

        val keyGenerator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        val builder = KeyGenParameterSpec.Builder(
            alias,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)

        if (requiresBiometric(alias)) {
            builder.setUserAuthenticationRequired(true)
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R) {
                builder.setUserAuthenticationParameters(
                    0,
                    KeyProperties.AUTH_BIOMETRIC_STRONG or KeyProperties.AUTH_DEVICE_CREDENTIAL
                )
            } else {
                builder.setUserAuthenticationValidityDurationSeconds(30)
            }
        }

        val strongBoxSupported = isStrongBoxSupported()
        if (strongBoxSupported) {
            try {
                builder.setIsStrongBoxBacked(true)
            } catch (_: Exception) {
                // Ignore if StrongBox is not available.
            }
        }

        try {
            keyGenerator.init(builder.build())
        } catch (_: Exception) {
            val fallbackBuilder = KeyGenParameterSpec.Builder(
                alias,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
            keyGenerator.init(fallbackBuilder.build())
        }

        return keyGenerator.generateKey()
    }

    private fun requiresBiometric(alias: String): Boolean {
        return alias == MASTER_KEY_ALIAS && isBiometricsEnabled()
    }

    private fun isBiometricsEnabled(): Boolean {
        val prefs = getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
        return prefs.getBoolean(PREFS_BIOMETRICS_ENABLED, false)
    }

    private fun setBiometricsEnabled(enabled: Boolean) {
        val prefs = getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
        prefs.edit().putBoolean(PREFS_BIOMETRICS_ENABLED, enabled).apply()
    }

    private fun encryptWithBiometric(alias: String, plaintext: ByteArray, result: MethodChannel.Result) {
        try {
            val secretKey = getOrCreateSecretKey(alias)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, secretKey)
            authenticateCipher(
                cipher,
                "Unlock Master Key",
                { authCipher ->
                    try {
                        val iv = authCipher.iv
                        val ciphertext = authCipher.doFinal(plaintext)
                        val out = ByteArray(iv.size + ciphertext.size)
                        System.arraycopy(iv, 0, out, 0, iv.size)
                        System.arraycopy(ciphertext, 0, out, iv.size, ciphertext.size)
                        result.success(out)
                    } catch (e: Exception) {
                        result.error("KEYSTORE_ERROR", e.message, null)
                    }
                },
                { errorMessage ->
                    result.error("AUTH_ERROR", errorMessage, null)
                }
            )
        } catch (e: Exception) {
            result.error("KEYSTORE_ERROR", e.message, null)
        }
    }

    private fun decryptWithBiometric(alias: String, sealed: ByteArray, result: MethodChannel.Result) {
        if (sealed.size < 13) {
            result.error("INVALID_ARGUMENT", "sealed data too short", null)
            return
        }
        try {
            val secretKey = getOrCreateSecretKey(alias)
            val iv = sealed.copyOfRange(0, 12)
            val ciphertext = sealed.copyOfRange(12, sealed.size)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, secretKey, GCMParameterSpec(128, iv))
            authenticateCipher(
                cipher,
                "Unlock Master Key",
                { authCipher ->
                    try {
                        val plaintext = authCipher.doFinal(ciphertext)
                        result.success(plaintext)
                    } catch (e: Exception) {
                        result.error("KEYSTORE_ERROR", e.message, null)
                    }
                },
                { errorMessage ->
                    result.error("AUTH_ERROR", errorMessage, null)
                }
            )
        } catch (e: Exception) {
            result.error("KEYSTORE_ERROR", e.message, null)
        }
    }

    private fun authenticateCipher(
        cipher: Cipher,
        title: String,
        onSuccess: (Cipher) -> Unit,
        onError: (String) -> Unit
    ) {
        val executor: Executor = ContextCompat.getMainExecutor(this)
        val promptInfo = BiometricPrompt.PromptInfo.Builder()
            .setTitle(title)
            .setSubtitle("Confirm to access secure keys")
            .setAllowedAuthenticators(
                BiometricManager.Authenticators.BIOMETRIC_STRONG or
                    BiometricManager.Authenticators.DEVICE_CREDENTIAL
            )
            .build()

        val biometricPrompt = BiometricPrompt(
            this,
            executor,
            object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                    val authCipher = result.cryptoObject?.cipher
                    if (authCipher != null) {
                        onSuccess(authCipher)
                    }
                }

                override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                    super.onAuthenticationError(errorCode, errString)
                    onError(errString.toString())
                }

            }
        )

        runOnUiThread {
            biometricPrompt.authenticate(promptInfo, BiometricPrompt.CryptoObject(cipher))
        }
    }

    private fun getCapabilities(): Map<String, Boolean> {
        return mapOf(
            "hasSecureHardware" to isStrongBoxSupported(),
            "hasStrongBox" to isStrongBoxSupported(),
            "hasSecureEnclave" to false,
            "hasBiometrics" to hasBiometrics()
        )
    }

    private fun isStrongBoxSupported(): Boolean {
        return android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.P &&
            packageManager.hasSystemFeature(android.content.pm.PackageManager.FEATURE_STRONGBOX_KEYSTORE)
    }

    private fun hasBiometrics(): Boolean {
        val manager = BiometricManager.from(this)
        return manager.canAuthenticate(
            BiometricManager.Authenticators.BIOMETRIC_STRONG
        ) == BiometricManager.BIOMETRIC_SUCCESS
    }
}

