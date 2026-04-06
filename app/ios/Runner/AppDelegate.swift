import UIKit
import Flutter
import BackgroundTasks
import UserNotifications
import Security
import LocalAuthentication

@UIApplicationMain
@objc class AppDelegate: FlutterAppDelegate {
    private var backgroundSyncChannel: FlutterMethodChannel?
    private var keystoreChannel: FlutterMethodChannel?
    private var securityChannel: FlutterMethodChannel?
    private let keystoreService = "com.pirate.wallet.keystore"
    private let masterKeyTag = "com.pirate.wallet.masterkey.secureenclave"
    private let masterKeyBiometricTag = "com.pirate.wallet.masterkey.secureenclave.biometric"
    private let biometricsPrefKey = "pirate_keystore_biometrics_enabled"
    private let screenShieldTag = 0x50525753
    private var screenShieldEnabled = false
    private var screenCaptureObserver: NSObjectProtocol?
    private var willResignObserver: NSObjectProtocol?
    private var didBecomeObserver: NSObjectProtocol?
    
    override func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {
        GeneratedPluginRegistrant.register(with: self)

        if let controller = window?.rootViewController as? FlutterViewController {
            let background = FlutterMethodChannel(
                name: "com.pirate.wallet/background_sync",
                binaryMessenger: controller.binaryMessenger
            )
            backgroundSyncChannel = background
            background.setMethodCallHandler { [weak self] call, result in
                guard let self = self else { return }
                self.handleBackgroundSyncCall(call: call, result: result)
            }

            let channel = FlutterMethodChannel(
                name: "com.pirate.wallet/keystore",
                binaryMessenger: controller.binaryMessenger
            )
            keystoreChannel = channel
            channel.setMethodCallHandler { [weak self] call, result in
                guard let self = self else { return }
                self.handleKeystoreCall(call: call, result: result)
            }

            let security = FlutterMethodChannel(
                name: "com.pirate.wallet/security",
                binaryMessenger: controller.binaryMessenger
            )
            securityChannel = security
            security.setMethodCallHandler { [weak self] call, result in
                guard let self = self else { return }
                self.handleSecurityCall(call: call, result: result)
            }
        }
        
        // Initialize background sync (iOS 13+)
        if #available(iOS 13.0, *) {
            setupBackgroundSync()
        }
        
        return super.application(application, didFinishLaunchingWithOptions: launchOptions)
    }
    
    // Handle background fetch (iOS 13+)
    override func application(
        _ application: UIApplication,
        performFetchWithCompletionHandler completionHandler: @escaping (UIBackgroundFetchResult) -> Void
    ) {
        print("[AppDelegate] Background fetch triggered")
        completionHandler(.noData)
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

    override func applicationWillResignActive(_ application: UIApplication) {
        if screenShieldEnabled {
            setScreenShieldVisible(true)
        }
    }
    
    // Handle entering foreground
    override func applicationWillEnterForeground(_ application: UIApplication) {
        print("[AppDelegate] App entering foreground")
        
        // Clear badge
        UIApplication.shared.applicationIconBadgeNumber = 0
    }

    override func applicationDidBecomeActive(_ application: UIApplication) {
        if screenShieldEnabled {
            updateScreenShieldVisibility()
        }
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

    private func handleBackgroundSyncCall(call: FlutterMethodCall, result: @escaping FlutterResult) {
        guard #available(iOS 13.0, *) else {
            result(FlutterError(code: "UNAVAILABLE", message: "Background sync requires iOS 13 or later", details: nil))
            return
        }

        switch call.method {
        case "initializeBackgroundSync":
            let args = call.arguments as? [String: Any]
            let compactIntervalMinutes = args?["compactIntervalMinutes"] as? Int
            let deepIntervalHours = args?["deepIntervalHours"] as? Int
            let maxCompactDurationSecs = args?["maxCompactDurationSecs"] as? Int
            let maxDeepDurationSecs = args?["maxDeepDurationSecs"] as? Int
            let maxCompactBlocks = args?["maxCompactBlocks"] as? Int
            let maxDeepBlocks = args?["maxDeepBlocks"] as? Int
            let requiresCharging = args?["requiresCharging"] as? Bool
            BackgroundSyncManager.shared.scheduleCompactSync(
                intervalMinutes: compactIntervalMinutes,
                maxDurationSecs: maxCompactDurationSecs,
                maxBlocks: maxCompactBlocks
            )
            BackgroundSyncManager.shared.scheduleDeepSync(
                intervalHours: deepIntervalHours,
                maxDurationSecs: maxDeepDurationSecs,
                maxBlocks: maxDeepBlocks,
                requiresCharging: requiresCharging
            )
            result(true)
        case "scheduleCompactSync":
            let args = call.arguments as? [String: Any]
            let intervalMinutes = args?["intervalMinutes"] as? Int
            let maxDurationSecs = args?["maxDurationSecs"] as? Int
            let maxBlocks = args?["maxBlocks"] as? Int
            BackgroundSyncManager.shared.scheduleCompactSync(
                intervalMinutes: intervalMinutes,
                maxDurationSecs: maxDurationSecs,
                maxBlocks: maxBlocks
            )
            result(true)
        case "scheduleDeepSync":
            let args = call.arguments as? [String: Any]
            let intervalHours = args?["intervalHours"] as? Int
            let maxDurationSecs = args?["maxDurationSecs"] as? Int
            let maxBlocks = args?["maxBlocks"] as? Int
            let requiresCharging = args?["requiresCharging"] as? Bool
            BackgroundSyncManager.shared.scheduleDeepSync(
                intervalHours: intervalHours,
                maxDurationSecs: maxDurationSecs,
                maxBlocks: maxBlocks,
                requiresCharging: requiresCharging
            )
            result(true)
        case "requestNotificationPermissions":
            Task {
                let granted = await BackgroundSyncManager.shared.requestNotificationPermissions()
                result(granted)
            }
        case "cancelAllTasks":
            BackgroundSyncManager.shared.cancelAllTasks()
            result(true)
        case "getSyncStatus":
            let status = BackgroundSyncManager.shared.getSyncStatus()
            let lastCompactSync: Any =
                status.lastCompactSync.map { Int($0.timeIntervalSince1970 * 1000) } ?? NSNull()
            let lastDeepSync: Any =
                status.lastDeepSync.map { Int($0.timeIntervalSince1970 * 1000) } ?? NSNull()
            result([
                "last_compact_sync": lastCompactSync,
                "last_deep_sync": lastDeepSync,
                "tunnel_mode": status.tunnelMode.rawValue,
                "is_running": false,
                "current_mode": NSNull()
            ])
        case "getTunnelMode":
            result(BackgroundSyncManager.shared.getTunnelConfig().mode.rawValue)
        case "setTunnelMode":
            let args = call.arguments as? [String: Any]
            let modeRaw = (args?["mode"] as? String) ?? "tor"
            let socks5Url = args?["socks5Url"] as? String
            let mode = BackgroundSyncManager.TunnelMode(rawValue: modeRaw) ?? .tor
            BackgroundSyncManager.shared.setTunnelMode(mode, socks5Url: socks5Url)
            result(true)
        case "setActiveWalletId":
            let args = call.arguments as? [String: Any]
            if let walletId = args?["walletId"] as? String, !walletId.isEmpty {
                BackgroundSyncManager.shared.setActiveWalletId(walletId)
            }
            result(true)
        case "clearActiveWalletId":
            BackgroundSyncManager.shared.clearActiveWalletId()
            result(true)
        case "pauseSync":
            BackgroundSyncManager.shared.setPaused(true)
            result(true)
        case "resumeSync":
            BackgroundSyncManager.shared.setPaused(false)
            result(true)
        case "triggerImmediateSync":
            let args = call.arguments as? [String: Any]
            let modeRaw = (args?["mode"] as? String) ?? "compact"
            let maxDurationSecs = args?["maxDurationSecs"] as? Int
            let maxBlocks = args?["maxBlocks"] as? Int
            Task {
                await BackgroundSyncManager.shared.runImmediateSync(
                    mode: modeRaw,
                    maxDurationSecs: maxDurationSecs,
                    maxBlocks: maxBlocks
                )
                result(true)
            }
        default:
            result(FlutterMethodNotImplemented)
        }
    }

    private func handleSecurityCall(call: FlutterMethodCall, result: @escaping FlutterResult) {
        switch call.method {
        case "enableScreenshotProtection":
            enableScreenShield(result: result)
        case "disableScreenshotProtection":
            disableScreenShield(result: result)
        default:
            result(FlutterMethodNotImplemented)
        }
    }

    private func enableScreenShield(result: @escaping FlutterResult) {
        DispatchQueue.main.async {
            self.screenShieldEnabled = true
            self.installScreenShieldIfNeeded()
            self.registerScreenShieldObserversIfNeeded()
            self.updateScreenShieldVisibility()
            result(true)
        }
    }

    private func disableScreenShield(result: @escaping FlutterResult) {
        DispatchQueue.main.async {
            self.screenShieldEnabled = false
            self.unregisterScreenShieldObservers()
            self.setScreenShieldVisible(false)
            result(true)
        }
    }

    private func installScreenShieldIfNeeded() {
        guard let window = self.window else { return }
        if window.viewWithTag(screenShieldTag) != nil {
            return
        }

        let blurView = UIVisualEffectView(effect: UIBlurEffect(style: .dark))
        blurView.tag = screenShieldTag
        blurView.frame = window.bounds
        blurView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        blurView.isUserInteractionEnabled = false
        blurView.isHidden = true
        window.addSubview(blurView)
    }

    private func setScreenShieldVisible(_ visible: Bool) {
        guard let window = self.window else { return }
        if let shield = window.viewWithTag(screenShieldTag) {
            shield.isHidden = !visible
        }
    }

    private func updateScreenShieldVisibility() {
        guard screenShieldEnabled else { return }
        if #available(iOS 11.0, *) {
            let isCaptured = UIScreen.main.isCaptured
            setScreenShieldVisible(isCaptured)
        } else {
            setScreenShieldVisible(false)
        }
    }

    private func registerScreenShieldObserversIfNeeded() {
        let center = NotificationCenter.default
        if #available(iOS 11.0, *), screenCaptureObserver == nil {
            screenCaptureObserver = center.addObserver(
                forName: UIScreen.capturedDidChangeNotification,
                object: nil,
                queue: .main
            ) { [weak self] _ in
                self?.updateScreenShieldVisibility()
            }
        }
        if willResignObserver == nil {
            willResignObserver = center.addObserver(
                forName: UIApplication.willResignActiveNotification,
                object: nil,
                queue: .main
            ) { [weak self] _ in
                self?.setScreenShieldVisible(true)
            }
        }
        if didBecomeObserver == nil {
            didBecomeObserver = center.addObserver(
                forName: UIApplication.didBecomeActiveNotification,
                object: nil,
                queue: .main
            ) { [weak self] _ in
                self?.updateScreenShieldVisibility()
            }
        }
    }

    private func unregisterScreenShieldObservers() {
        let center = NotificationCenter.default
        if let observer = screenCaptureObserver {
            center.removeObserver(observer)
            screenCaptureObserver = nil
        }
        if let observer = willResignObserver {
            center.removeObserver(observer)
            willResignObserver = nil
        }
        if let observer = didBecomeObserver {
            center.removeObserver(observer)
            didBecomeObserver = nil
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
