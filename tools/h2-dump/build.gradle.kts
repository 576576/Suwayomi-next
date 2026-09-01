plugins {
    kotlin("jvm") version "2.2.20"
    application
}

repositories {
    mavenCentral()
}

dependencies {
    implementation("com.h2database:h2:2.3.232")
}

application {
    mainClass.set("h2dump.MainKt")
}

kotlin {
    // 与 jvm-sandbox 一致：统一 Java 25（本地/CI 均为 Temurin 25）
    jvmToolchain(25)
}

tasks.jar {
    manifest {
        attributes["Main-Class"] = "h2dump.MainKt"
    }
    from(configurations.runtimeClasspath.get().map { if (it.isDirectory) it else zipTree(it) })
    duplicatesStrategy = DuplicatesStrategy.EXCLUDE
}
