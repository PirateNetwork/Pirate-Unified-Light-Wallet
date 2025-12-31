import UIKit
import Flutter
import BackgroundTasks
import UserNotifications
import Security
import LocalAuthentication

@UIApplicationMain
@objc class AppDelegate: FlutterAppDelegate {
    private var keystoreChannel: FlutterMethodChannel?
    private let keystoreService = "com.pirate.wallet.keystore"
    private let masterKeyTag = "com.pirate.wallet.masterkey.secureenclave"
    private let masterKeyBiometricTag = "com.pirate.wallet.masterkey.secureenclave.biometric"
    private let biometricsPrefKey = "pirate_keystore_biometrics_enabled"
    
    override func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        GeneratedPluginRegistrant.register(with: self)

        if let controller = window?.rootViewController as? FlutterViewController {
            let channel = FlutterMethodChannel(
                name: "com.pirate.wallet/keystore",
                binaryMessenger: controller.binaryMessenger
            )
            keystoreChannel = channel
            channel.setMethodCallHandler { [weak self] call, result in
                guard let self = self else { return }
                self.handleKeystoreCall(call: call, result: result)
            }
        }
        
        // Initialize background sync (iOS 13+)
        if #available(iOS 13.0, *) {
            setupBackgroundSync()
        }
        
        return super.application(application, didFinishLaunchingWithOptions: launchOptions)
    }
    
    // Handle background fetch (iOS 13+)
    @available(iOS 13.0, *)
    override func application(
        _ application: UIApplication,
        performFetchWithCompletionHandler completionHandler: @escaping (UIBackgroundFetchResult) -> Void
    ) {
        // This is called when system wakes app for background refresh
        print("[AppDelegate] Background fetch triggered")
        
        // Trigger compact sync
        BackgroundSyncManager.shared.scheduleCompactSync()
        
        completionHandler(.newData)
    }
    
    // Handle entering background
    override func applicationDidEnterBackground(_ application: UIApplication) {
        if #available(iOS 13.0, *) {
            // Schedule background tasks when app enters background
            BackgroundSyncManager.shared.scheduleCompactSync()
            BackgroundSyncManager.shared.scheduleDeepSync()
            
            print("[AppDelegate] Scheduled background sync tasks")
        }
    }
    
    // Handle entering foreground
    override func applicationWillEnterForeground(_ application: UIApplication) {
        print("[AppDelegate] App entering foreground")
        
        // Clear badge
        UIApplication.shared.applicationIconBadgeNumber = 0
    }

    private func handleKeystoreCall(call: FlutterMethodCall, result: @escaping FlutterResult) {
        switch call.method {
        case "storeKey":
            guard let args = call.arguments as? [String: Any],
                  let keyId = args["keyId"] as? String,
                  let encryptedKey = args["encryptedKey"] as? FlutterStandardTypedData else {
                result(FlutterError(code: "INVALID_ARGUMENT", message: "keyId and encryptedKey required", details: nil))
                return
            }
            do {
                try storeKeychainData(encryptedKey.data, account: keyId)
                result(true)
            } catch {
                result(FlutterError(code: "KEYSTORE_ERROR", message: error.localizedDescription, details: nil))
            }
        case "retrieveKey":
            guard let args = call.arguments as? [String: Any],
                  let keyId = args["keyId"] as? String else {
                result(FlutterError(code: "INVALID_ARGUMENT", message: "keyId required", details: nil))
                return
            }
            do {
                if let data = try loadKeychainData(account: keyId) {
                    result(FlutterStandardTypedData(bytes: data))
                } else {
                    result(nil)
                }
            } catch {
                result(FlutterError(code: "KEYSTORE_ERROR", message: error.localizedDescription, details: nil))
            }
        case "deleteKey":
            guard let args = call.arguments as? [String: Any],
                  let keyId = args["keyId"] as? String else {
                result(FlutterError(code: "INVALID_ARGUMENT", message: "keyId required", details: nil))
                return
            }
            do {
                try deleteKeychainData(account: keyId)
                result(true)
            } catch {
                result(FlutterError(code: "KEYSTORE_ERROR", message: error.localizedDescription, details: nil))
            }
        case "keyExists":
            guard let args = call.arguments as? [String: Any],
                  let keyId = args["keyId"] as? String else {
                result(FlutterError(code: "INVALID_ARGUMENT", message: "keyId required", details: nil))
                return
            }
            result(keyExists(account: keyId))
        case "sealMasterKey":
            guard let args = call.arguments as? [String: Any],
                  let masterKey = args["masterKey"] as? FlutterStandardTypedData else {
                result(FlutterError(code: "INVALID_ARGUMENT", message: "masterKey required", details: nil))
                return
            }
            do {
                let sealed = try sealMasterKey(masterKey.data)
                result(FlutterStandardTypedData(bytes: sealed))
            } catch {
                result(FlutterError(code: "SEAL_ERROR", message: error.localizedDescription, details: nil))
            }
        case "unsealMasterKey":
            guard let args = call.arguments as? [String: Any],
                  let sealedKey = args["sealedKey"] as? FlutterStandardTypedData else {
                result(FlutterError(code: "INVALID_ARGUMENT", message: "sealedKey required", details: nil))
                return
            }
            do {
                let unsealed = try unsealMasterKey(sealedKey.data)
                result(FlutterStandardTypedData(bytes: unsealed))
            } catch {
                result(FlutterError(code: "UNSEAL_ERROR", message: error.localizedDescription, details: nil))
            }
        case "getCapabilities":
            result(getCapabilities())
        case "setBiometricsEnabled":
            guard let args = call.arguments as? [String: Any],
                  let enabled = args["enabled"] as? Bool else {
                result(FlutterError(code: "INVALID_ARGUMENT", message: "enabled required", details: nil))
                return
            }
            setBiometricsEnabled(enabled)
            result(true)
        default:
            result(FlutterMethodNotImplemented)
        }
    }

    private func storeKeychainData(_ data: Data, account: String) throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: keystoreService,
            kSecAttrAccount as String: account,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
            kSecValueData as String: data
        ]

        let status = SecItemAdd(query as CFDictionary, nil)
        if status == errSecDuplicateItem {
            let update: [String: Any] = [
                kSecValueData as String: data
            ]
            let queryUpdate: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: keystoreService,
                kSecAttrAccount as String: account
            ]
            let updateStatus = SecItemUpdate(queryUpdate as CFDictionary, update as CFDictionary)
            if updateStatus != errSecSuccess {
                throw NSError(domain: "KeystoreError", code: Int(updateStatus), userInfo: nil)
            }
        } else if status != errSecSuccess {
            throw NSError(domain: "KeystoreError", code: Int(status), userInfo: nil)
        }
    }

    private func loadKeychainData(account: String) throws -> Data? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: keystoreService,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        if status == errSecItemNotFound {
            return nil
        }
        if status != errSecSuccess {
            throw NSError(domain: "KeystoreError", code: Int(status), userInfo: nil)
        }
        return (item as? Data)
    }

    private func deleteKeychainData(account: String) throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: keystoreService,
            kSecAttrAccount as String: account
        ]
        let status = SecItemDelete(query as CFDictionary)
        if status != errSecSuccess && status != errSecItemNotFound {
            throw NSError(domain: "KeystoreError", code: Int(status), userInfo: nil)
        }
    }

    private func keyExists(account: String) -> Bool {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: keystoreService,
            kSecAttrAccount as String: account,
            kSecReturnData as String: false,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]
        let status = SecItemCopyMatching(query as CFDictionary, nil)
        return status == errSecSuccess
    }

    private func sealMasterKey(_ masterKey: Data) throws -> Data {
        let privateKey = try getOrCreateSecureEnclaveKey(requiresBiometrics: isBiometricsEnabled())
        guard let publicKey = SecKeyCopyPublicKey(privateKey) else {
            throw NSError(domain: "KeystoreError", code: -1, userInfo: [NSLocalizedDescriptionKey: "Failed to get public key"])
        }
        var error: Unmanaged<CFError>?
        guard let ciphertext = SecKeyCreateEncryptedData(
            publicKey,
            .eciesEncryptionStandardVariableIVX963SHA256AESGCM,
            masterKey as CFData,
            &error
        ) else {
            throw error?.takeRetainedValue() ?? NSError(domain: "KeystoreError", code: -1, userInfo: [NSLocalizedDescriptionKey: "Failed to seal key"])
        }
        return ciphertext as Data
    }

    private func unsealMasterKey(_ sealedKey: Data) throws -> Data {
        let privateKey = try getOrCreateSecureEnclaveKey(requiresBiometrics: isBiometricsEnabled())
        var error: Unmanaged<CFError>?
        guard let plaintext = SecKeyCreateDecryptedData(
            privateKey,
            .eciesEncryptionStandardVariableIVX963SHA256AESGCM,
            sealedKey as CFData,
            &error
        ) else {
            throw error?.takeRetainedValue() ?? NSError(domain: "KeystoreError", code: -1, userInfo: [NSLocalizedDescriptionKey: "Failed to unseal key"])
        }
        return plaintext as Data
    }

    private func getOrCreateSecureEnclaveKey(requiresBiometrics: Bool) throws -> SecKey {
        let tagString = requiresBiometrics ? masterKeyBiometricTag : masterKeyTag
        let tag = tagString.data(using: .utf8)!
        let context = LAContext()
        context.localizedReason = "Unlock Master Key"
        let query: [String: Any] = [
            kSecClass as String: kSecClassKey,
            kSecAttrApplicationTag as String: tag,
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeyClass as String: kSecAttrKeyClassPrivate,
            kSecReturnRef as String: true,
            kSecUseAuthenticationContext as String: context
        ]
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        if status == errSecSuccess, let key = item as! SecKey? {
            return key
        }

        var flags: SecAccessControlCreateFlags = [.privateKeyUsage]
        if requiresBiometrics {
            flags.insert(.biometryCurrentSet)
        }
        let access = SecAccessControlCreateWithFlags(
            nil,
            kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
            flags,
            nil
        )
        var attributes: [String: Any] = [
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeySizeInBits as String: 256,
            kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
            kSecPrivateKeyAttrs as String: [
                kSecAttrIsPermanent as String: true,
                kSecAttrApplicationTag as String: tag,
                kSecAttrAccessControl as String: access as Any
            ]
        ]

        var error: Unmanaged<CFError>?
        guard let key = SecKeyCreateRandomKey(attributes as CFDictionary, &error) else {
            throw error?.takeRetainedValue() ?? NSError(domain: "KeystoreError", code: -1, userInfo: [NSLocalizedDescriptionKey: "Failed to create Secure Enclave key"])
        }
        return key
    }

    private func isBiometricsEnabled() -> Bool {
        return UserDefaults.standard.bool(forKey: biometricsPrefKey)
    }

    private func setBiometricsEnabled(_ enabled: Bool) {
        UserDefaults.standard.set(enabled, forKey: biometricsPrefKey)
    }

    private func getCapabilities() -> [String: Bool] {
        let context = LAContext()
        var error: NSError?
        let hasBiometrics = context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error)
        let hasSecureEnclave = secureEnclaveAvailable()
        return [
            "hasSecureHardware": hasSecureEnclave,
            "hasStrongBox": false,
            "hasSecureEnclave": hasSecureEnclave,
            "hasBiometrics": hasBiometrics
        ]
    }

    private func secureEnclaveAvailable() -> Bool {
        guard #available(iOS 13.0, *) else { return false }
        #if targetEnvironment(simulator)
        return false
        #else
        let attributes: [String: Any] = [
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeySizeInBits as String: 256,
            kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
            kSecAttrIsPermanent as String: false
        ]
        var error: Unmanaged<CFError>?
        return SecKeyCreateRandomKey(attributes as CFDictionary, &error) != nil
        #endif
    }
}

