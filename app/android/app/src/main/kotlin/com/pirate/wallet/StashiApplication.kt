package com.pirate.wallet

import android.app.Application
import android.content.Context

class StashiApplication : Application() {
    override fun attachBaseContext(base: Context) {
        super.attachBaseContext(base)
        NativeRuntimeEnvironment.configure(this)
    }
}
