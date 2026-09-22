package sandbox

import kotlin.test.Test
import kotlin.test.assertEquals

/**
 * 老扩展模板用相对包名声明 Source 类（`android:value=".CopyManga"`）——不拼包名的话
 * `Class.forName` 会失败，整个扩展加载不了（issue #8 之后那个 copy manga 的复现）。
 */
class SourceClassNameTest {

    @Test
    fun relativeNameIsQualifiedWithPackage() {
        assertEquals(
            "eu.kanade.tachiyomi.extension.zh.copymanga.CopyManga",
            resolveSourceClass("eu.kanade.tachiyomi.extension.zh.copymanga", ".CopyManga"),
        )
    }

    @Test
    fun fullyQualifiedNameIsKept() {
        assertEquals(
            "keiyoushi.source.Generated",
            resolveSourceClass("eu.kanade.tachiyomi.extension.all.komga", "keiyoushi.source.Generated"),
        )
    }

    @Test
    fun surroundingWhitespaceIsIgnored() {
        assertEquals("p.Foo", resolveSourceClass("p", "  .Foo  "))
    }
}
