import java.security.MessageDigest
import java.util.Properties
import java.util.zip.ZipFile

plugins {
    java
}

// 本子项目不编译任何源码，jar 的内容全部来自 AOSP 包。这里的 target 只用于依赖解析的
// 兼容性判定：声明 21（与 android-compat 一致），25 的消费者也能用。不声明的话会默认取
// 构建 JVM（25），android-compat 解析 compileClasspath 时会被判为不兼容。
java {
    sourceCompatibility = JavaVersion.VERSION_21
    targetCompatibility = JavaVersion.VERSION_21
}

// 公开 API 基线（AOSP target），pin 在 android-stub.properties 里。
val pin = Properties().also { props ->
    file("android-stub.properties").inputStream().use(props::load)
}
val apiLevel = pin.getProperty("aospApiLevel")
val packageRevision = pin.getProperty("aospPlatformPackageRevision")
val packageSha256 = pin.getProperty("aospPlatformPackageSha256")
val jarSha256 = pin.getProperty("aospPlatformJarSha256")
val prebuiltsCommit = pin.getProperty("aospPrebuiltsCommit")
val stripRevision = pin.getProperty("stripRevision")

// 版本号 = <api>.<包修订>.<剥离修订>，直接对应具体是哪份 AOSP 公开 API 基线。
val stubVersion = "$apiLevel.$packageRevision.$stripRevision"
val packageName = "platform-${apiLevel}_${packageRevision}.zip"

val aospDir = layout.buildDirectory.dir("aosp")
val aospPackage = aospDir.map { it.file(packageName) }
val aospJar = aospDir.map { it.file("android.jar") }

// 官方源在部分网络不可达时可换镜像，pin（sha256）仍然生效。
val aospPackageUrl = providers.gradleProperty("aospPackageUrl")
    .orElse("https://dl.google.com/android/repository/$packageName")

fun sha256(file: File): String {
    val digest = MessageDigest.getInstance("SHA-256")
    file.inputStream().use { input ->
        val buffer = ByteArray(1 shl 20)
        while (true) {
            val read = input.read(buffer)
            if (read < 0) break
            digest.update(buffer, 0, read)
        }
    }
    return digest.digest().joinToString("") { "%02x".format(it) }
}

// 与上游 getAndroid.sh 一致：这几个前缀是核心库而不是 android.* 桩，留在 classpath 上会遮蔽 JDK。
val coreLibPrefixes = listOf(
    "java", "javax", "org/apache", "org/json", "org/w3c", "org/xml", "org/xmlpull", "junit",
)

// android-compat 自己实现的类必须从桩里剔掉：fat jar 里同名类两份时，谁生效取决于
// classpath 顺序，两份的实现可以不一致。推导方式同 getAndroid.sh —— 按源码文件路径
// 算类名，目录名里的点（如 rx.android.schedulers）也要还原成包路径。
val compatSourceDirs = listOf(
    file("../android-compat/src/main/java"),
    file("../android-compat/config/src/main/java"),
    file("../../extension-runtime/src/main/kotlin"),
)

fun dedupPatterns(): List<String> {
    val names = sortedSetOf<String>()
    compatSourceDirs.filter { it.isDirectory }.forEach { dir ->
        dir.walkTopDown()
            .filter { it.isFile && (it.name.endsWith(".java") || it.name.endsWith(".kt")) }
            .forEach { source ->
                val stem = source.relativeTo(dir).path
                    .replace(File.separatorChar, '/')
                    .substringBeforeLast('.')
                names += stem
                names += stem.replace('.', '/')
                // Kotlin 顶层函数落在 <文件名>Kt.class 上
                names += "${stem}Kt"
            }
    }
    // 内部类与匿名类以 <外层类>$ 开头，按前缀一并剔除
    return names.flatMap { listOf("$it.class", "$it\$*.class") }
}

val prepareAospJar by tasks.registering {
    group = "android-stub"
    description = "下载 AOSP 公开 API 包并解出 android.jar"
    inputs.property("packageName", packageName)
    inputs.property("packageSha256", packageSha256)
    inputs.property("jarSha256", jarSha256)
    inputs.property("packageUrl", aospPackageUrl)
    outputs.file(aospPackage)
    outputs.file(aospJar)

    doLast {
        val downloaded = aospPackage.get().asFile
        if (!downloaded.isFile || sha256(downloaded) != packageSha256) {
            downloaded.parentFile.mkdirs()
            val partial = File(downloaded.parentFile, "${downloaded.name}.part")
            logger.lifecycle("android-stub: 下载 ${aospPackageUrl.get()}")
            uri(aospPackageUrl.get()).toURL().openStream().use { input ->
                partial.outputStream().use { output -> input.copyTo(output) }
            }
            val actual = sha256(partial)
            check(actual == packageSha256) {
                "AOSP 包 sha256 不符：期望 $packageSha256，实得 $actual"
            }
            partial.renameTo(downloaded)
        }

        val jar = aospJar.get().asFile
        ZipFile(downloaded).use { archive ->
            val entry = archive.entries().asSequence().first { it.name.endsWith("android.jar") }
            archive.getInputStream(entry).use { input ->
                jar.outputStream().use { output -> input.copyTo(output) }
            }
        }
        val actual = sha256(jar)
        check(actual == jarSha256) {
            "android.jar sha256 不符：期望 $jarSha256，实得 $actual"
        }
    }
}

// 基线信息随产物走：fat jar 会把本子项目的类摊平打包，本子项目自己的 manifest 会丢，
// 但 META-INF 下的资源不会 —— 想知道某个 jvm-sandbox.jar 是用哪份 AOSP 基线构建的，
// 直接 unzip -p bin/jvm-sandbox.jar META-INF/android-stub.properties。
val provenanceFile = layout.buildDirectory.file("android-stub-provenance.properties")
val writeProvenance by tasks.registering(WriteProperties::class) {
    destinationFile.set(provenanceFile)
    comment = "android-stub public API baseline"
    property("version", stubVersion)
    property("aosp.api.level", apiLevel)
    property("aosp.platform.package", packageName)
    property("aosp.platform.package.sha256", packageSha256)
    property("aosp.platform.jar.sha256", jarSha256)
    property("aosp.prebuilts.commit", prebuiltsCommit)
    property("stub.strip.revision", stripRevision)
}

tasks.jar {
    dependsOn(prepareAospJar)
    dependsOn(writeProvenance)
    archiveFileName.set("android-stub-$stubVersion.jar")
    inputs.dir(file("../android-compat/src/main/java"))
    inputs.dir(file("../android-compat/config/src/main/java"))
    inputs.dir(file("../../extension-runtime/src/main/kotlin"))

    val patterns = dedupPatterns()
    from(aospJar.map { project.zipTree(it.asFile) }) {
        exclude(coreLibPrefixes.flatMap { listOf("$it/**", "$it/") })
        exclude(patterns)
        // 本任务自己写 manifest，不带 AOSP 包里那份
        exclude("META-INF/MANIFEST.MF")
    }
    from(provenanceFile) {
        into("META-INF")
        rename { "android-stub.properties" }
    }

    manifest {
        attributes(
            "Implementation-Title" to "android-stub",
            "Implementation-Version" to stubVersion,
            "Aosp-Api-Level" to apiLevel,
            "Aosp-Platform-Package" to packageName,
            "Aosp-Prebuilts-Commit" to prebuiltsCommit,
            "Stub-Strip-Revision" to stripRevision,
        )
    }
}
