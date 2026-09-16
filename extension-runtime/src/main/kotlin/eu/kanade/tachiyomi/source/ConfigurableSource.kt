package eu.kanade.tachiyomi.source

import android.content.SharedPreferences
import androidx.preference.PreferenceScreen

interface ConfigurableSource : Source {
    /**
     * Gets instance of [SharedPreferences] scoped to the specific source.
     *
     * @since extensions-lib 1.5
     */
    fun getSourcePreferences(): SharedPreferences = InMemoryPreferences(preferenceKey())

    fun setupPreferenceScreen(screen: PreferenceScreen)
}

fun ConfigurableSource.preferenceKey(): String = "source_$id"

fun ConfigurableSource.sourcePreferences(): SharedPreferences = InMemoryPreferences(preferenceKey())

fun sourcePreferences(key: String): SharedPreferences = InMemoryPreferences(key)

/** Minimal in-memory SharedPreferences stub (sandbox has no Android storage). */
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
