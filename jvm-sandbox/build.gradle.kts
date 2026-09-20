import java.util.zip.ZipFile

plugins {
    id("org.jetbrains.kotlin.jvm")
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

    // --- Android stub API ---
    // AndroidCompat（android.* / androidx.* 的桌面桩实现）连同它的 Config 模块以源码形式随本仓库
    // 构建，见 android-compat/ —— 改这些桩不必再去改 jar。两个子项目把三方依赖声明为 compileOnly，
    // 运行期由本模块提供（同名坐标在下面已列出）。
    implementation(project(":android-compat"))
    implementation(project(":android-compat:config"))
    // AOSP 公开 API 空壳，构建期按 android-stub/ 的 pin 生成（已剔除 AndroidCompat 自己实现的类，
    // 与本工程零重名）。扩展用到的 `android.*` 远不止 AndroidCompat 那 626 个类：源设置界面会链到
    // `android.widget.TextView` / `android.text.TextWatcher` / `android.icu.text.*` 等等，
    // 缺一个整个设置页就是 NoClassDefFoundError。方法体一律是
    // `throw new RuntimeException("Stub!")`，只在**链接期**被用到，不会被调用。
    implementation(project(":android-stub"))
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


// fat jar 里同名类两份时，谁生效取决于 classpath 顺序，两份实现可以不一致。android-stub 的
// 剥离集合是按源码路径推导的，这里核对实际产物，避免推导漏项静默变成运行时行为差异。
val verifyStubDedup by tasks.registering {
    group = "verification"
    description = "核对 android-stub 与 android-compat 没有同名类"
    val stubJar = project(":android-stub").tasks.named<Jar>("jar").flatMap { it.archiveFile }
    val compatJar = project(":android-compat").tasks.named<Jar>("jar").flatMap { it.archiveFile }
    inputs.files(stubJar, compatJar)

    doLast {
        fun classes(path: File): Set<String> = ZipFile(path).use { archive ->
            archive.entries().asSequence()
                .map { it.name }
                .filter { it.endsWith(".class") }
                .toSet()
        }

        val duplicates = classes(stubJar.get().asFile) intersect classes(compatJar.get().asFile)
        check(duplicates.isEmpty()) {
            "android-stub 与 android-compat 有 ${duplicates.size} 个同名类，例如 ${duplicates.first()}"
        }
        logger.lifecycle("android-stub: 与 android-compat 无同名类")
    }
}

tasks.jar {
    dependsOn(verifyStubDedup)
    manifest {
        attributes["Main-Class"] = "sandbox.MainKt"
    }
    // fat jar: bundle kotlin-stdlib + runtime deps so the jar runs standalone
    from(configurations.runtimeClasspath.get().map { if (it.isDirectory) it else zipTree(it) }) {
        exclude("META-INF/*.SF", "META-INF/*.DSA", "META-INF/*.RSA")
    }
    duplicatesStrategy = DuplicatesStrategy.EXCLUDE
}
