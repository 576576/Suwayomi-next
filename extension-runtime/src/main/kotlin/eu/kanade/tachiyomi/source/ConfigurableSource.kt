package eu.kanade.tachiyomi.source

import android.content.SharedPreferences
import androidx.preference.PreferenceScreen
import java.util.concurrent.ConcurrentHashMap

interface ConfigurableSource : Source {
    /**
     * Gets instance of [SharedPreferences] scoped to the specific source.
     *
     * @since extensions-lib 1.5
     */
    fun getSourcePreferences(): SharedPreferences = sourcePreferences(preferenceKey())

    fun setupPreferenceScreen(screen: PreferenceScreen)
}

fun ConfigurableSource.preferenceKey(): String = "source_$id"

fun ConfigurableSource.sourcePreferences(): SharedPreferences = sourcePreferences(preferenceKey())

/**
 * 同一个 key 恒返回同一个实例。
 *
 * 每次调用都新建的话，写入和读取会落在两份互不相干的内存里：扩展在
 * `setupPreferenceScreen` 里存下的账号密码，业务代码再 `sourcePreferences()`
 * 永远读不到 —— 表现为「设置填了、请求仍说未登录」。
 *
 * 想让它跨进程重启保留，宿主先用 [PreferenceStores.installFactory] 换实现
 * （桌面沙盒换成落盘版）；不换则退化为进程内存储。
 */
fun sourcePreferences(key: String): SharedPreferences = PreferenceStores.of(key)

object PreferenceStores {
    private val stores = ConcurrentHashMap<String, SharedPreferences>()

    @Volatile
    private var factory: (String) -> SharedPreferences = { InMemoryPreferences(it) }

    /** 换实现。已建好的实例一并丢弃，之后按新实现重建。 */
    fun installFactory(f: (String) -> SharedPreferences) {
        stores.clear()
        factory = f
    }

    fun of(key: String): SharedPreferences = stores.computeIfAbsent(key) { factory(it) }
}

/** In-memory SharedPreferences stub, used when the host has no storage. */
class InMemoryPreferences(private val key: String) : SharedPreferences {
    private val store = java.util.concurrent.ConcurrentHashMap<String, Any?>()

    override fun getAll(): MutableMap<String, *> = store

    override fun getString(k: String, def: String?): String? = store[k] as? String ?: def

    override fun getStringSet(k: String, def: MutableSet<String>?): MutableSet<String>? =
        (store[k] as? Set<*>)?.map { it.toString() }?.toMutableSet() ?: def

    override fun getInt(k: String, def: Int): Int = (store[k] as? Number)?.toInt() ?: def

    override fun getLong(k: String, def: Long): Long = (store[k] as? Number)?.toLong() ?: def

    override fun getFloat(k: String, def: Float): Float = (store[k] as? Number)?.toFloat() ?: def

    override fun getBoolean(k: String, def: Boolean): Boolean = store[k] as? Boolean ?: def

    override fun contains(k: String): Boolean = store.containsKey(k)

    override fun edit(): SharedPreferences.Editor = object : SharedPreferences.Editor {
        override fun putString(k: String, v: String?): SharedPreferences.Editor = apply { store[k] = v }
        override fun putStringSet(k: String, v: MutableSet<String>?): SharedPreferences.Editor = apply { store[k] = v }
        override fun putInt(k: String, v: Int): SharedPreferences.Editor = apply { store[k] = v }
        override fun putLong(k: String, v: Long): SharedPreferences.Editor = apply { store[k] = v }
        override fun putFloat(k: String, v: Float): SharedPreferences.Editor = apply { store[k] = v }
        override fun putBoolean(k: String, v: Boolean): SharedPreferences.Editor = apply { store[k] = v }
        override fun remove(k: String): SharedPreferences.Editor = apply { store.remove(k) }
        override fun clear(): SharedPreferences.Editor = apply { store.clear() }
        override fun commit(): Boolean = true
        override fun apply() {}
    }

    override fun registerOnSharedPreferenceChangeListener(l: SharedPreferences.OnSharedPreferenceChangeListener?) {}
    override fun unregisterOnSharedPreferenceChangeListener(l: SharedPreferences.OnSharedPreferenceChangeListener?) {}
}
