plugins {
    id("org.jetbrains.kotlin.jvm")
}

repositories {
    mavenCentral()
    google()
    // android.jar 空壳（官方 SDK stub，上游剔重后发布在这条分支上）。
    maven("https://github.com/Suwayomi/Suwayomi-Server/raw/android-jar/")
}

dependencies {
    implementation(project(":android-compat:config"))

    // 编译期需要的 android.* / androidx.* 桩。运行期由应用模块把同一个坐标打进 fat jar ——
    // 扩展会链到 AndroidCompat 没实现的类（android.widget.TextView 等），缺了就 NoClassDefFoundError。
    compileOnly("com.github.Suwayomi:android-jar:1.0.0")

    // 运行期需要的：坐标与 jvm-sandbox/build.gradle.kts 里的一致，Gradle 去重后 fat jar 不变。
    implementation("com.typesafe:config:1.4.9")
    implementation("io.github.config4k:config4k:0.7.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.11.0")
    implementation("io.insert-koin:koin-core:3.5.6")
    implementation("io.github.oshai:kotlin-logging-jvm:6.0.9")
    implementation("org.slf4j:slf4j-api:2.0.13")
    implementation("ca.gosyer:kotlin-multiplatform-appdirs:2.0.0")
    implementation("org.jsoup:jsoup:1.18.1")
    implementation("io.reactivex:rxjava:1.3.8")
    implementation("net.dongliu:apk-parser:2.6.10")
    implementation("de.femtopedia.dex2jar:dex-tools:2.4.38")
    // app.cash.quickjs.QuickJs 的实现换成 Rhino（上游用 graalvm polyglot，为它要多背 67MB）。
    implementation("org.mozilla:rhino:1.8.0")

    // 编译期用得到、运行期走不到的地方：注解，以及 replace/java/**（无任何外部引用）、
    // JavaSharedPreferences（无任何外部引用）这两处死代码路径。
    compileOnly("org.jetbrains:annotations:26.0.2")
    compileOnly("androidx.annotation:annotation:1.10.0")
    compileOnly("com.fasterxml.jackson.core:jackson-annotations:2.22")
    compileOnly("com.android.tools.build:apksig:9.4.0")
    compileOnly("com.ibm.icu:icu4j:78.3")
    // android.jar 剔掉了 org.xmlpull；android/os/PersistableBundle 与 XmlUtils 要它。
    compileOnly("xmlpull:xmlpull:1.1.3.4a")
    compileOnly("com.russhwolf:multiplatform-settings-jvm:1.3.0")
    compileOnly("com.russhwolf:multiplatform-settings-serialization-jvm:1.3.0")
}

java {
    sourceCompatibility = JavaVersion.VERSION_21
    targetCompatibility = JavaVersion.VERSION_21
}

kotlin {
    jvmToolchain(25)
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_21)
    }
}
