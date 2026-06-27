plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "com.spiritchat"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.spiritchat"
        // API 33+ for AGSL shaders + real-time RenderEffect blur (Liquid Glass).
        minSdk = 33
        targetSdk = 35
        // CI passes the run number so each push gets a higher versionCode,
        // letting the in-app updater install a newer build over the old one.
        versionCode = (System.getenv("VERSION_CODE") ?: "1").toInt()
        versionName = System.getenv("VERSION_NAME") ?: "0.1.0"
    }

    signingConfigs {
        // A fixed key committed to the repo so every build (CI included) is
        // signed identically — required for in-app updates to install over the
        // previous build. Fine for a sideloaded dev app; a Play release would
        // use a secret-based key instead.
        create("shared") {
            storeFile = file("spiritchat.keystore")
            storePassword = "spiritchat"
            keyAlias = "spiritchat"
            keyPassword = "spiritchat"
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("shared")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.09.03")
    implementation(composeBom)

    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-compose:1.9.2")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.6")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.6")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("dev.chrisbanes.haze:haze:1.7.2")
    implementation("dev.chrisbanes.haze:haze-materials:1.7.2")
}
