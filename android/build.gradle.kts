// 只声明 AGP 版本，不声明 Kotlin 插件：
// AGP 9 起**内置 Kotlin 支持**，再显式应用 `org.jetbrains.kotlin.android`
// 会被它直接拒绝（"no longer required for Kotlin support since AGP 9.0"）。
plugins {
    id("com.android.application") version "9.2.1" apply false
    id("com.android.library") version "9.2.1" apply false
}
