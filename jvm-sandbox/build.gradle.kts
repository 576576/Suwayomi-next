plugins {
    kotlin("jvm") version "2.4.0"
    application
}

repositories {
    mavenCentral()
    maven("https://jitpack.io")
}

dependencies {
    testImplementation(kotlin("test"))
    // --- extension runtime (provided by the sandbox process) ---
    implementation("com.squareup.okhttp3:okhttp:5.5.0")
    implementation("com.squareup.okhttp3:okhttp-brotli:5.5.0")
    implementation("com.squareup.okhttp3:okhttp-zstd:5.5.0")
    implementation("com.squareup.okio:okio:3.9.0")
    implementation("org.jsoup:jsoup:1.18.1")
    implementation("com.google.code.gson:gson:2.11.0")
    implementation("io.reactivex.rxjava2:rxjava:2.2.21")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
    // 必须 >= 1.11.0：keiyoushi 扩展是按 1.11 编译的，它们在源里直接调 `runBlocking`，
    // 编译产物指向 `BuildersKt.runBlockingK(...)`，而这个方法 1.9/1.10 里还没有
    // （只有 `runBlocking`），低版本会在请求时抛 NoSuchMethodError。
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.11.0")
    // mangaplus 这类扩展用 protobuf 编码读站点接口（`kotlinx/serialization/protobuf/ProtoNumber`）。
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-protobuf:1.11.0")
    implementation("com.squareup.moshi:moshi:1.15.1")
    implementation("com.squareup.moshi:moshi-kotlin:1.15.1")
    implementation("io.insert-koin:koin-core:3.5.6")
    implementation("com.squareup.okhttp3:logging-interceptor:5.5.0")
    implementation("io.github.oshai:kotlin-logging-jvm:6.0.9")
    implementation("org.slf4j:slf4j-api:2.0.13")
    implementation("com.github.null2264:injekt-koin:ee267b2e27")
    implementation("org.ow2.asm:asm:9.7.1")
    runtimeOnly("org.slf4j:slf4j-nop:2.0.13")
    implementation("io.reactivex:rxjava:1.3.8")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json-okio:1.11.0")
    implementation("io.insert-koin:koin-core:3.5.6")
    implementation("com.squareup.okhttp3:logging-interceptor:5.5.0")
    implementation("io.github.oshai:kotlin-logging-jvm:6.0.9")
    implementation("org.slf4j:slf4j-api:2.0.13")
    runtimeOnly("org.slf4j:slf4j-nop:2.0.13")
    implementation("io.reactivex:rxjava:1.3.8")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json-okio:1.11.0")

    // --- apk -> dex -> jar toolchain ---
    implementation("de.femtopedia.dex2jar:dex-tools:2.4.38")
    implementation("net.dongliu:apk-parser:2.6.10")

    // --- Android stub API (from the AndroidCompat project) ---
    // Android stub API (compiled with Kotlin 2.4, same as the sandbox compiler)
    implementation(files("libs/AndroidCompat-1.0.jar"))
    // 官方 android.jar 空壳（上游 `com.github.Suwayomi:android-jar`，已剔除 AndroidCompat
    // 自己实现的类，与本 jar 零重名）。扩展用到的 `android.*` 远不止 AndroidCompat 那 757 个类：
    // 源设置界面会链到 `android.widget.TextView` / `android.text.TextWatcher` /
    // `android.icu.text.*` 等等，缺一个整个设置页就是 NoClassDefFoundError。
    // 方法体一律是 `throw new RuntimeException("Stub!")`，只在**链接期**被用到，不会被调用。
    // 不引 jitpack（`com.github.Suwayomi:android-jar:1.0.0`）：那条坐标现在回 401，
    // 构建会卡在依赖解析上。
    implementation(files("libs/android.jar"))
    // AndroidCompat 的 `android.os.Build` / `SystemProperties` 静态依赖 `xyz.nulldev.ts.config`，
    // 那套配置子系统在 AndroidCompat 仓里是独立的 Config 模块（上游由 server 侧的
    // `implementation(projects.androidCompat.config)` 提供），不打进 AndroidCompat-1.0.jar。
    // 缺了它，凡是在 `headersBuilder()` 里读 `Build.X` 的扩展都会倒在 CNFE 上。
    implementation(files("libs/Config-1.0.jar"))
    implementation("com.typesafe:config:1.4.9")
    implementation("io.github.config4k:config4k:0.7.0")
    implementation("ca.gosyer:kotlin-multiplatform-appdirs:2.0.0")
}

application {
    mainClass.set("sandbox.MainKt")
}

kotlin {
    // 统一 Java 25：与 CI setup-java（Temurin 25）及发布捆绑的 JRE 25 一致
    jvmToolchain(25)
    // 共享源码树：`eu.kanade.tachiyomi.**` 接口实现、SourceDriver、Router 等
    // 与平台无关的部分由桌面沙盒和 Android extension-host 共同编译，
    // 避免两份实现漂移（见 docs/migration/ANDROID_IMPL.md A4）。
    sourceSets["main"].kotlin.srcDir("../extension-runtime/src/main/kotlin")
}

tasks.test {
    useJUnitPlatform()
    // AndroidCompat 类编译为 Java 21（major 65）；沙盒运行时用 JAVA_HOME（>=21）。
    // 测试任务用 JVM 25（toolchain 统一），避免 UnsupportedClassVersionError。
    javaLauncher.set(javaToolchains.launcherFor {
        languageVersion.set(JavaLanguageVersion.of(25))
    })
}


tasks.jar {
    manifest {
        attributes["Main-Class"] = "sandbox.MainKt"
    }
    // fat jar: bundle kotlin-stdlib + runtime deps so the jar runs standalone
    from(configurations.runtimeClasspath.get().map { if (it.isDirectory) it else zipTree(it) }) {
        exclude("META-INF/*.SF", "META-INF/*.DSA", "META-INF/*.RSA")
    }
    duplicatesStrategy = DuplicatesStrategy.EXCLUDE
}
