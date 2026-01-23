import Cocoa
import FlutterMacOS
import Security
import LocalAuthentication

@main
class AppDelegate: FlutterAppDelegate {
  private let keystoreService = "com.pirate.wallet.keystore"
  private let masterKeyAccount = "pirate_wallet_master_key"
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
        try storeKeychainData(masterKey.data, account: masterKeyAccount)
        result(FlutterStandardTypedData(bytes: Data()))
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
        if let data = try loadKeychainData(account: masterKeyAccount) {
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
