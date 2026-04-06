package com.pirate.wallet.reactnative

import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod

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
}
