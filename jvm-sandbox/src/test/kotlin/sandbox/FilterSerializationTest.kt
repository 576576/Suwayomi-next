package sandbox

import eu.kanade.tachiyomi.source.model.Filter
import kotlin.test.Test
import kotlin.test.assertEquals

/**
 * Verifies filter serialization against the Filter stub hierarchy
 * (`eu.kanade.tachiyomi.source.model.Filter`).
 */
class FilterSerializationTest {

    @Test
    fun serializesAllFilterTypes() {
        val text = object : Filter.Text("关键词", "漫") {}
        assertEquals(
            mapOf("type" to "text", "name" to "关键词", "state" to "漫"),
            filterToMap(text),
        )

        val check = object : Filter.CheckBox("只看已汉化", true) {}
        assertEquals(
            mapOf("type" to "check-box", "name" to "只看已汉化", "state" to true),
            filterToMap(check),
        )

        val tri = object : Filter.TriState("已读状态", Filter.TriState.STATE_EXCLUDE) {}
        assertEquals(
            mapOf("type" to "tri-state", "name" to "已读状态", "state" to 2),
            filterToMap(tri),
        )

        val select = object : Filter.Select<String>("排序", arrayOf("新更", "热门"), 1) {}
        assertEquals(
            mapOf("type" to "select", "name" to "排序", "state" to 1, "values" to listOf("新更", "热门")),
            filterToMap(select),
        )

        val sort = object : Filter.Sort("排列", arrayOf("评分", "日期"), Filter.Sort.Selection(0, false)) {}
        assertEquals(
            mapOf(
                "type" to "sort",
                "name" to "排列",
                "values" to listOf("评分", "日期"),
                "state" to mapOf("index" to 0, "ascending" to false),
            ),
            filterToMap(sort),
        )

        val header = Filter.Header("标签")
        assertEquals(mapOf("type" to "title", "name" to "标签"), filterToMap(header))

        val group = object : Filter.Group<String>("类型", listOf("动作", "奇幻")) {}
        assertEquals(
            mapOf("type" to "group", "name" to "类型", "state" to listOf("动作", "奇幻")),
            filterToMap(group),
        )
    }

    @Test
    fun nullSafe() {
        assertEquals(emptyMap<String, Any?>(), filterToMap(null))
    }
}
/** Verifies the Android Context stub serves real dirs (keiyoushi lib1.6 needs
 * context.getCacheDir()/getFilesDir() in getFilterList). */
class SandboxContextTest {
    @Test
    fun contextOnceProvidesDirs() {
        val app = SandboxApp()
        kotlin.test.assertTrue(app.getCacheDir().isDirectory)
        kotlin.test.assertTrue(app.getFilesDir().isDirectory)
        val sp = app.getSharedPreferences("x", 0)
        val ed = sp.edit()
        ed.putString("k", "v")
        ed.apply()
        kotlin.test.assertEquals("v", sp.getString("k", null))
    }
}
