// :app —— Android 宿主本体。
//
// 只做三件桌面端由 CLI 与打包脚本负责的事：
//  1. 把 Rust server（jniLibs/arm64-v8a/libsuwayomi_android.so）拉起来（JNI，同进程）；
//  2. 把打包进 assets 的 WebUI 解到 filesDir 再交给 server；
//  3. 用 WebView 打开 127.0.0.1:<port>，以及把「安装/卸载扩展」交给系统安装器。
//
// 刻意**不引 AppCompat / Material / Compose**：界面就是一个全屏 WebView，
// 引这些只会让 APK 变大并带来主题约束。

plugins {
    id("com.android.application")
}

android {
    namespace = "suwayomi.android"
    // 与 :extension-host 一致（okhttp-android 的 AAR metadata 要求 37）
    compileSdk = 37

    defaultConfig {
        applicationId = "suwayomi.android"
        // 与 NDK 交叉编译用的 API level（aarch64-linux-android26-clang）保持一致；
        // 26 同时满足 java.nio.file（扩展运行时用到）与 dalvik DelegateLastClassLoader 的下限。
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"
    }

    // release 签名：CI 通过环境变量注入 keystore（见 .github/workflows/build.yml 的
    // "准备 release 签名（可选）" 步）。
    //
    // 没注入时回退 AGP 自带的 debug key —— 自用分发里"能装上"比"签名好看"重要。
    // 代价要说清楚：CI runner 每次都是全新的，AGP 现场生成的 debug key 每次都不同，
    // 所以**跨次覆盖安装前要先卸载**。要稳定签名就配 ANDROID_KEYSTORE_BASE64 那组 secret。
    val ksPath = System.getenv("SUWAYOMI_KEYSTORE").orEmpty()
    val hasReleaseKey = ksPath.isNotEmpty()
    if (hasReleaseKey) {
        signingConfigs {
            create("release") {
                storeFile = rootProject.file(ksPath)
                storePassword = System.getenv("SUWAYOMI_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("SUWAYOMI_KEY_ALIAS") ?: "suwayomi"
                keyPassword = System.getenv("SUWAYOMI_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig =
                if (hasReleaseKey) signingConfigs.getByName("release") else signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    // Rust cdylib 放 src/main/jniLibs/<abi>/libsuwayomi_android.so
    // （AGP 默认就收这个目录，由 scripts/build-rust.sh 拷贝，不进仓库）
    androidResources {
        // WebUI 包本身已经是 zip，别再压一次（解压时能按 stored 直读，省内存）
        noCompress += "zip"
    }
}

// 同 :extension-host：AGP 内置 Kotlin 2.3.20 需要显式打开 context parameters
kotlin {
    compilerOptions {
        freeCompilerArgs.add("-Xcontext-parameters")
    }
}

dependencies {
    implementation(project(":extension-host"))
    implementation("androidx.annotation:annotation:1.9.1")
    // FileProvider：Android 7+ 不允许用 file:// 把 APK 交给系统安装器
    implementation("androidx.core:core:1.15.0")
}
