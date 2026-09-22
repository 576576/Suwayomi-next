package sandbox

import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * 搬进 jar 的资源：`assets/` 下的文件 + 根目录普通文件（第三方库按绝对路径读的字典表）；
 * dex 与 Android 自己的东西不搬。
 */
class JarResourceTest {

    @Test
    fun copiesAssetsAndRootLevelFiles() {
        assertTrue(isJarResource("assets/i18n/messages_zh.properties"))
        assertTrue(isJarResource("simp.txt"))
        assertTrue(isJarResource("simplified.txt"))
    }

    @Test
    fun skipsAndroidArtifacts() {
        assertFalse(isJarResource("classes.dex"))
        assertFalse(isJarResource("classes2.dex"))
        assertFalse(isJarResource("AndroidManifest.xml"))
        assertFalse(isJarResource("resources.arsc"))
        assertFalse(isJarResource("res/drawable/ic_launcher.png"))
        assertFalse(isJarResource("lib/arm64-v8a/libfoo.so"))
        assertFalse(isJarResource("META-INF/MANIFEST.MF"))
    }
}
