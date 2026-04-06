import Foundation
import PirateWalletNative
import React

@objc(PirateWalletReactNative)
final class PirateWalletReactNative: NSObject {
  @objc
  static func requiresMainQueueSetup() -> Bool {
    false
  }

  @objc
  func invoke(
    _ requestJson: String,
    pretty: Bool,
    resolver resolve: @escaping RCTPromiseResolveBlock,
    rejecter reject: @escaping RCTPromiseRejectBlock
  ) {
    guard let request = requestJson.cString(using: .utf8) else {
      reject("PIRATE_WALLET_INVOKE_ERROR", "Request string was not valid UTF-8.", nil)
      return
    }

    let pointer = request.withUnsafeBufferPointer { buffer in
      pirate_wallet_service_invoke_json(buffer.baseAddress, pretty)
    }

    guard let pointer else {
      reject("PIRATE_WALLET_INVOKE_ERROR", "Wallet service returned a null response.", nil)
      return
    }

    defer {
      pirate_wallet_service_free_string(pointer)
    }

    let response = String(cString: pointer)
    resolve(response)
  }
}
