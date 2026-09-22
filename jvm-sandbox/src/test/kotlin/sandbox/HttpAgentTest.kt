package sandbox

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * 桌面 JVM 没有 AndroidRuntime 设的 `http.agent`，扩展 `System.getProperty("http.agent")`
 * 拿到 null 会在构造期 NPE（copy manga 的 `headersBuilder()`）。
 */
class HttpAgentTest {

    @Test
    fun fillsMissingAgentAndKeepsExisting() {
        val original = System.getProperty("http.agent")
        try {
            System.clearProperty("http.agent")
            installHttpAgent()
            assertTrue(
                System.getProperty("http.agent")!!.startsWith("Dalvik/"),
                "应填 Android 默认那种形状的 UA：${System.getProperty("http.agent")}",
            )

            System.setProperty("http.agent", "custom")
            installHttpAgent()
            assertEquals("custom", System.getProperty("http.agent"), "已经有值就不该覆盖")
        } finally {
            if (original == null) System.clearProperty("http.agent") else System.setProperty("http.agent", original)
        }
    }
}
