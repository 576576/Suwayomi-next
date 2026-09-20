plugins {
    id("org.jetbrains.kotlin.jvm")
}

repositories {
    mavenCentral()
}

dependencies {
    // 只用于编译。运行时这些类由应用模块的 classpath 提供 —— 本模块产出的 jar 只含自己的类。
    compileOnly("com.typesafe:config:1.4.9")
    compileOnly("io.github.config4k:config4k:0.7.0")
    compileOnly("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
    compileOnly("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.11.0")
    compileOnly("io.insert-koin:koin-core:3.5.6")
    compileOnly("io.github.oshai:kotlin-logging-jvm:6.0.9")
    compileOnly("org.slf4j:slf4j-api:2.0.13")
    // Logging.kt 配的是 logback；应用模块用 slf4j-nop，这个入口运行期不会被走到。
    compileOnly("ch.qos.logback:logback-classic:1.6.3")
    compileOnly("ca.gosyer:kotlin-multiplatform-appdirs:2.0.0")
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
