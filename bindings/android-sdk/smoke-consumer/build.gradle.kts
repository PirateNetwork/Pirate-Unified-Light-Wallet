plugins {
    id("com.android.application")
    kotlin("android")
}

import org.jetbrains.kotlin.gradle.dsl.JvmTarget

android {
    namespace = "com.pirate.wallet.sdk.smoke"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.pirate.wallet.sdk.smoke"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }

    kotlin {
        compilerOptions {
            jvmTarget.set(JvmTarget.JVM_1_8)
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }
}

dependencies {
    implementation(project(":"))
    implementation(kotlin("stdlib"))
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.1")
}
