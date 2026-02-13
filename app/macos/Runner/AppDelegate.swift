import Cocoa
import FlutterMacOS
import Security
import LocalAuthentication

@main
class AppDelegate: FlutterAppDelegate {
  private let keystoreService = "com.pirate.wallet.keystore"
  private let masterKeyAccount = "pirate_wallet_master_key"
  private let masterKeyPrompt = "Authenticate to unlock Pirate Wallet"
  private let sealedKeyMarker = Data("macos-keychain-v1".utf8)
  private var securityChannel: FlutterMethodChannel?

  override func applicationDidFinishLaunching(_ notification: Notification) {
    if let controller = mainFlutterWindow?.contentViewController as? FlutterViewController {
      let channel = FlutterMethodChannel(
        name: "com.pirate.wallet/keystore",
        binaryMessenger: controller.engine.binaryMessenger
      )
      channel.setMethodCallHandler { [weak self] call, result in
        guard let self = self else { return }
        self.handleKeystoreCall(call: call, result: result)
      }

      let security = FlutterMethodChannel(
        name: "com.pirate.wallet/security",
        binaryMessenger: controller.engine.binaryMessenger
      )
      securityChannel = security
      security.setMethodCallHandler { [weak self] call, result in
        guard let self = self else { return }
        switch call.method {
        case "enableScreenshotProtection":
          DispatchQueue.main.async {
            self.mainFlutterWindow?.sharingType = .none
            result(true)
          }
        case "disableScreenshotProtection":
          DispatchQueue.main.async {
            self.mainFlutterWindow?.sharingType = .readOnly
            result(true)
          }
        default:
          result(FlutterMethodNotImplemented)
        }
      }
    }

    super.applicationDidFinishLaunching(notification)
  }

  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
    return true
  }

  override func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool {
    return true
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
        // Bind the cached passphrase secret to biometric auth at the Keychain
        // layer. This is defense-in-depth beyond app-level local_auth checks.
        try storeKeychainData(
          masterKey.data,
          account: masterKeyAccount,
          requireBiometric: true
        )
        // Return a non-empty marker so Dart-side cache existence checks work.
        result(FlutterStandardTypedData(bytes: sealedKeyMarker))
      } catch {
        result(FlutterError(code: "SEAL_ERROR", message: error.localizedDescription, details: nil))
      }
    case "unsealMasterKey":
      guard let args = call.arguments as? [String: Any],
            let _ = args["sealedKey"] as? FlutterStandardTypedData else {
        result(FlutterError(code: "INVALID_ARGUMENT", message: "sealedKey required", details: nil))
        return
      }
      do {
        if let data = try loadKeychainData(
          account: masterKeyAccount,
          requireBiometric: true
        ) {
          result(FlutterStandardTypedData(bytes: data))
        } else {
          result(FlutterError(code: "UNSEAL_ERROR", message: "Master key not found", details: nil))
        }
      } catch {
        result(FlutterError(code: "UNSEAL_ERROR", message: error.localizedDescription, details: nil))
      }
    case "getCapabilities":
      result(getCapabilities())
    default:
      result(FlutterMethodNotImplemented)
    }
  }

  private func storeKeychainData(
    _ data: Data,
    account: String,
    requireBiometric: Bool = false
  ) throws {
    let baseQuery: [String: Any] = [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: keystoreService,
      kSecAttrAccount as String: account
    ]

    var addQuery = baseQuery
    addQuery[kSecValueData as String] = data

    if requireBiometric {
      var accessError: Unmanaged<CFError>?
      guard let accessControl = SecAccessControlCreateWithFlags(
        nil,
        kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
        .biometryCurrentSet,
        &accessError
      ) else {
        let code: Int
        if let error = accessError?.takeRetainedValue() {
          code = CFErrorGetCode(error)
        } else {
          code = Int(errSecParam)
        }
        throw NSError(domain: "KeystoreError", code: code, userInfo: nil)
      }
      addQuery[kSecAttrAccessControl as String] = accessControl
    } else {
      addQuery[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
    }

    // Recreate item so its ACL matches current policy (biometric vs standard).
    let deleteStatus = SecItemDelete(baseQuery as CFDictionary)
    if deleteStatus != errSecSuccess && deleteStatus != errSecItemNotFound {
      throw NSError(domain: "KeystoreError", code: Int(deleteStatus), userInfo: nil)
    }

    let status = SecItemAdd(addQuery as CFDictionary, nil)
    if status != errSecSuccess {
      throw NSError(domain: "KeystoreError", code: Int(status), userInfo: nil)
    }
  }

  private func loadKeychainData(
    account: String,
    requireBiometric: Bool = false
  ) throws -> Data? {
    var query: [String: Any] = [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: keystoreService,
      kSecAttrAccount as String: account,
      kSecReturnData as String: true,
      kSecMatchLimit as String: kSecMatchLimitOne
    ]
    if requireBiometric {
      query[kSecUseOperationPrompt as String] = masterKeyPrompt
    }
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
    let attributes: [String: Any] = [
      kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
      kSecAttrKeySizeInBits as String: 256,
      kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
      kSecAttrIsPermanent as String: false
    ]
    var error: Unmanaged<CFError>?
    return SecKeyCreateRandomKey(attributes as CFDictionary, &error) != nil
  }
}
