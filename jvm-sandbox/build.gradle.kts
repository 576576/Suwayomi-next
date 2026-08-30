plugins {
    kotlin("jvm") version "2.2.20"
    application
}

repositories {
    mavenCentral()
}

dependencies {
    // Minimal sandbox: uses only the JDK built-in HTTP server.
    // Full extension interface integration (eu.kanade.tachiyomi.source.* +
    // AndroidCompat) lands as Phase 5 continues; the HTTP contract below is
    // stable and independent of the interface strategy.
}

application {
    mainClass.set("sandbox.MainKt")
}

kotlin {
    jvmToolchain(17)
}

tasks.jar {
    manifest {
        attributes["Main-Class"] = "sandbox.MainKt"
    }
    // fat jar: bundle kotlin-stdlib so the jar runs standalone
    from(configurations.runtimeClasspath.get().map { if (it.isDirectory) it else zipTree(it) })
    duplicatesStrategy = DuplicatesStrategy.EXCLUDE
}
