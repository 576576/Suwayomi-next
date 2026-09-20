//! 落盘版 [SharedPreferences]。
//!
//! 两端共用（只依赖 Android 标准的 `SharedPreferences` 接口，不碰 `androidx.preference`）：
//! 扩展填进去的账号 / 服务器地址要能跨进程重启读回来，只放内存里则重启即丢。
//!
//! 只支持 `String` / `Boolean` / `Set<String>` —— 这正是扩展设置项的全部取值类型。

package sandbox

import android.content.SharedPreferences
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonPrimitive
import java.nio.file.Files
import java.nio.file.Path
import java.util.Properties
import java.util.concurrent.ConcurrentHashMap

/**
 * 一个 key 一份 `.properties` 文件，值带类型前缀存盘 —— 否则读回来分不清
 * `"true"` 是字符串还是布尔。
 */
class FilePreferences(private val file: Path) : SharedPreferences {
    private val store = ConcurrentHashMap<String, Any>()
    private val listeners = ConcurrentHashMap<SharedPreferences.OnSharedPreferenceChangeListener, (String) -> Unit>()

    init {
        load()
    }

    private fun load() {
        if (!Files.isRegularFile(file)) return
        try {
            val props = Properties()
            Files.newInputStream(file).use { props.load(it) }
            props.stringPropertyNames().forEach { k ->
                decode(props.getProperty(k))?.let { store[k] = it }
            }
        } catch (t: Throwable) {
            System.err.println("sandbox: cannot read preferences $file: $t")
        }
    }

    private fun save() {
        try {
            val props = Properties()
            store.forEach { (k, v) -> encode(v)?.let { props.setProperty(k, it) } }
            file.parent?.let { Files.createDirectories(it) }
            Files.newOutputStream(file).use { props.store(it, null) }
        } catch (t: Throwable) {
            System.err.println("sandbox: cannot write preferences $file: $t")
        }
    }

    private fun encode(v: Any?): String? = when (v) {
        null -> null
        is Set<*> -> "l" + JsonArray(v.map { JsonPrimitive(it.toString()) })
        is Boolean -> "b$v"
        else -> "s$v"
    }

    private fun decode(raw: String?): Any? = when {
        raw == null -> null
        raw.startsWith("l") -> Json.parseToJsonElement(raw.substring(1)).jsonArray.map { it.jsonPrimitive.content }.toMutableSet()
        raw.startsWith("b") -> raw.substring(1).toBoolean()
        raw.startsWith("s") -> raw.substring(1)
        else -> raw
    }

    override fun getAll(): MutableMap<String, *> = HashMap(store)

    override fun getString(k: String, def: String?): String? = store[k] as? String ?: def

    override fun getStringSet(k: String, def: MutableSet<String>?): MutableSet<String>? =
        (store[k] as? Set<*>)?.map { it.toString() }?.toMutableSet() ?: def

    override fun getInt(k: String, def: Int): Int = (store[k] as? Number)?.toInt() ?: def

    override fun getLong(k: String, def: Long): Long = (store[k] as? Number)?.toLong() ?: def

    override fun getFloat(k: String, def: Float): Float = (store[k] as? Number)?.toFloat() ?: def

    override fun getBoolean(k: String, def: Boolean): Boolean = store[k] as? Boolean ?: def

    override fun contains(k: String): Boolean = store.containsKey(k)

    override fun edit(): SharedPreferences.Editor = Editor()

    private inner class Editor : SharedPreferences.Editor {
        private val pending = LinkedHashMap<String, Any?>()
        private val removals = mutableListOf<String>()
        private var clearAll = false

        override fun putString(k: String, v: String?): SharedPreferences.Editor = apply { stage(k, v) }
        override fun putStringSet(k: String, v: MutableSet<String>?): SharedPreferences.Editor = apply { stage(k, v) }
        override fun putInt(k: String, v: Int): SharedPreferences.Editor = apply { stage(k, v) }
        override fun putLong(k: String, v: Long): SharedPreferences.Editor = apply { stage(k, v) }
        override fun putFloat(k: String, v: Float): SharedPreferences.Editor = apply { stage(k, v) }
        override fun putBoolean(k: String, v: Boolean): SharedPreferences.Editor = apply { stage(k, v) }
        override fun remove(k: String): SharedPreferences.Editor = apply { removals.add(k) }
        override fun clear(): SharedPreferences.Editor = apply { clearAll = true }

        private fun stage(k: String, v: Any?) {
            pending[k] = v
            removals.remove(k)
        }

        override fun commit(): Boolean {
            applyPending()
            return true
        }

        override fun apply() {
            applyPending()
        }

        private fun applyPending() {
            // 扩展建设置项与宿主写值可能并发（主 looper 线程 + HTTP 请求线程），
            // 不加锁会读到写了一半的状态。
            synchronized(store) {
                if (clearAll) store.clear()
                removals.forEach { store.remove(it) }
                pending.forEach { (k, v) -> if (v == null) store.remove(k) else store[k] = v }
            }
            save()
            (pending.keys + removals).forEach { k -> listeners.values.forEach { it(k) } }
        }
    }

    override fun registerOnSharedPreferenceChangeListener(l: SharedPreferences.OnSharedPreferenceChangeListener?) {
        if (l == null) return
        listeners[l] = { key -> l.onSharedPreferenceChanged(this, key) }
    }

    override fun unregisterOnSharedPreferenceChangeListener(l: SharedPreferences.OnSharedPreferenceChangeListener?) {
        if (l == null) return
        listeners.remove(l)
    }
}
