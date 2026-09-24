// :extension-host —— Android 侧扩展宿主。
//
// 与桌面 ext-runtime 的关系（见 docs/migration/ANDROID_IMPL.md D2/D3）：
//  * **共享**：ext-runtime 发布的共享源码包，展开到 `android/build/ext-runtime-src`
//    —— `eu.kanade.tachiyomi.**`
//    接口实现、SourceDriver、Router、JSON 契约。同一份源码，两端编译。
//  * **差异**：扩展怎么被发现与加载。桌面是 目录扫描 → dex2jar → ASM 修字节码 →
//    自建 ClassLoader；这里是 PackageManager → dalvik ClassLoader 直接吃 APK 的 dex。
//    因此本模块**不含** dex-tools / apk-parser / ASM / AndroidCompat：这些是
//    「在桌面 JVM 里假装成 Android」的补丁，在真机上既不需要也会与系统框架抢类。

plugins {
    id("com.android.library")
}

android {
    namespace = "suwayomi.extension.host"
    // 37 不是随便挑的：okhttp 5.x 的 Android 变体 `okhttp-android` 声明了
    // `compileSdkVersion=37` 的 AAR metadata，36 会在 checkDebugAarMetadata 直接失败。
    compileSdk = 37

    defaultConfig {
        minSdk = 26
    }

    // Kotlin 由 AGP 9 内置（不再应用 org.jetbrains.kotlin.android）
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

// 共享源码树里 OkHttpExtensions.kt 用了 `context(_: Json)` —— Kotlin 的
// context parameters。AGP 9.2.1 内置的 Kotlin 是 **2.3.20**（ext-runtime 用的是
// 2.4.0，2.4 起默认不再报错），2.3 下必须显式打开这个 feature flag。
// 这里给 flag 而不是改写共享源码：那份代码是桌面沙盒已在真实扩展上验证过的，
// 保持两端逐字一致比省一个编译器参数更重要。
kotlin {
    compilerOptions {
        freeCompilerArgs.add("-Xcontext-parameters")
    }
}

// ext-runtime 的共享源码树：由 `android/scripts/fetch-ext-runtime-src.sh` 从发布资产
// 下载并展开到 `android/build/ext-runtime-src`（下载产物，不进版本库）。
//
// 配置期就校验，而不是等编译报「找不到符号」：三个包根是本模块的全部输入，
// 少任何一个都说明下载/展开环节出了问题，那时报错该指向原因而不是后果。
val extRuntimeSrcDir: File = rootProject.layout.buildDirectory.dir("ext-runtime-src").get().asFile

require(extRuntimeSrcDir.isDirectory) {
    """
    |缺少 ext-runtime 共享源码目录：$extRuntimeSrcDir
    |先执行：bash android/scripts/fetch-ext-runtime-src.sh
    """.trimMargin()
}
for (pkg in listOf("eu/kanade/tachiyomi", "suwayomi/tachidesk", "sandbox")) {
    require(File(extRuntimeSrcDir, pkg).isDirectory) {
        "共享源码目录缺少 $pkg/：$extRuntimeSrcDir（展开不完整？）"
    }
}

// 不用 `android.sourceSets.getByName("main").kotlin.srcDir(...)`：
// AGP 9 下这条老路会抛 ClassCastException —— `DefaultAndroidLibrarySourceSet`
// 实现的是新的 `com.android.build.api.dsl.AndroidLibrarySourceSet`，而
// `sourceSets` 这个 Kotlin DSL 访问器仍按 `com.android.build.gradle.api.AndroidLibrarySourceSet`
// 去强转（两个同名不同包的接口），无论 `kotlin` 还是 `java` 都绕不开。
// 改用 AGP 的变体 Sources API —— 这才是 9.x 支持的加源目录方式。
androidComponents {
    onVariants { variant ->
        variant.sources.kotlin?.addStaticSourceDirectory(
            extRuntimeSrcDir.absolutePath,
        )
    }
}

dependencies {
    // `ConfigurableSource.setupPreferenceScreen(screen: PreferenceScreen)` 是本模块
    // 编译期就需要的签名（共享源码树里也有 typealias），因此必须进依赖。
    implementation("androidx.preference:preference:1.2.1")
    implementation("androidx.annotation:annotation:1.9.1")

    // --- 扩展 API 运行时（与 ext-runtime 保持一致：扩展不自带第三方库，全部由宿主提供）---
    implementation("com.squareup.okhttp3:okhttp:5.5.0")
    implementation("com.squareup.okhttp3:okhttp-brotli:5.5.0")
    implementation("com.squareup.okhttp3:okhttp-zstd:5.5.0")
    implementation("com.squareup.okhttp3:logging-interceptor:5.5.0")
    implementation("com.squareup.okio:okio:3.9.0")
    implementation("org.jsoup:jsoup:1.18.1")
    implementation("com.google.code.gson:gson:2.11.0")
    implementation("io.reactivex.rxjava2:rxjava:2.2.21")
    implementation("io.reactivex:rxjava:1.3.8")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json-okio:1.11.0")
    // 与 ext-runtime 同版本：扩展编译产物调的是 `BuildersKt.runBlockingK(...)`，
    // 它 1.11.0 才出现，低版本在请求时抛 NoSuchMethodError。
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-protobuf:1.11.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.11.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")
    implementation("com.squareup.moshi:moshi:1.15.1")
    implementation("com.squareup.moshi:moshi-kotlin:1.15.1")
    implementation("io.insert-koin:koin-core:3.5.6")
    implementation("io.github.oshai:kotlin-logging-jvm:6.0.9")
    implementation("org.slf4j:slf4j-api:2.0.13")
    implementation("org.slf4j:slf4j-nop:2.0.13")
    // injekt 桥：扩展用 injektLazy()/Injekt.get<T>() 取依赖，宿主必须提供实例
    implementation("com.github.null2264:injekt-koin:ee267b2e27")
}
