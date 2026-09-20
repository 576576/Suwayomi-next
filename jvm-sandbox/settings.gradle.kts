pluginManagement {
    plugins {
        id("org.jetbrains.kotlin.jvm") version "2.4.0"
    }
}

rootProject.name = "suwayomi-jvm-sandbox"

// 上游 AndroidCompat（android.* / androidx.* 的桌面桩实现）连同它的 Config 子模块，以源码形式
// 随本仓库构建 —— 改行为不必再去改 jar。两个子项目只把三方依赖声明为 compileOnly，
// 产出的 jar 只含自己的类。
include("android-compat")
include("android-compat:config")
