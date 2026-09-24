// Android 宿主工程。与 ext-runtime 一样是**独立**的 Gradle 构建（不并入主工程），
// 原因见 docs/migration/ANDROID_IMPL.md：它只在打 Android 包时才需要，
// 且需要 Google Maven 与 Android SDK，桌面用户不该为此付出任何代价。

pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
        // injekt-koin（扩展的依赖注入桥）只发布在 JitPack
        maven(url = "https://jitpack.io")
    }
}

rootProject.name = "suwayomi-android"

include(":app")
include(":extension-host")
