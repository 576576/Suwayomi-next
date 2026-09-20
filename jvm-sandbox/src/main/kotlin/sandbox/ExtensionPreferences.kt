//! 源设置（`ConfigurableSource`）的序列化与写回 —— 桌面专有。
//!
//! 不放共享源码树：`androidx.preference.Preference` 在两端不是同一个东西 —— 桌面上是
//! AndroidCompat 的桩，带 `getCurrentValue` / `saveNewValue` / `getDefaultValueType` 这套
//! Tachidesk 扩展 API；Android 上是真 AndroidX，取值要走各控件自己的
//! `isChecked` / `getText` / `getValue`。硬凑一份代码得先堆一层反射兼容分支。

package sandbox

import android.content.Context
import android.content.SharedPreferences
import androidx.preference.CheckBoxPreference
import androidx.preference.EditTextPreference
import androidx.preference.ListPreference
import androidx.preference.MultiSelectListPreference
import androidx.preference.Preference
import androidx.preference.PreferenceScreen
import androidx.preference.SwitchPreferenceCompat
import androidx.preference.TwoStatePreference
import eu.kanade.tachiyomi.source.sourcePreferences
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonPrimitive

/**
 * 能被 WebUI 渲染的偏好项，顺序即位置索引。
 *
 * `PreferenceCategory` / 裸 `Preference` 不在其中：它们只有标题、没有值。序列化与写值
 * 都必须走这个函数 —— 拿 `screen.preferences` 直接当索引，一旦有项被跳过，前端传回来的
 * 位置就会改到别的项上。
 */
fun preferenceList(screen: PreferenceScreen): List<Preference> =
    screen.preferences.filter {
        it is MultiSelectListPreference || it is ListPreference || it is EditTextPreference || it is TwoStatePreference
    }

/** 序列化成 WebUI `SOURCE_SETTING_FIELDS` 认得的形状（`type` 即 union 成员名）。 */
fun preferenceScreenToJson(screen: PreferenceScreen): String =
    "[" + preferenceList(screen).joinToString(",") { preferenceToJson(it) } + "]"

private fun preferenceToJson(p: Preference): String {
    ensureDefault(p)
    val common = """"key":${jsonStr(p.key ?: "")},"title":${jsonStr(p.title?.toString() ?: "")}""" +
        ""","summary":${jsonStr(p.summary?.toString() ?: "")},"visible":${p.visible},"enabled":${p.isEnabled}"""
    return when (p) {
        is MultiSelectListPreference -> {
            val current = (p.currentValue as? Collection<*>)?.map { it.toString() }.orEmpty()
            val default = (p.defaultValue as? Collection<*>)?.map { it.toString() }.orEmpty()
            """{"type":"MultiSelectListPreference",$common""" +
                ""","dialogTitle":${dialogTitleJson(p)}""" +
                ""","dialogMessage":${jsonStr(p.dialogMessage?.toString() ?: "")}""" +
                ""","entries":${charSeqArrayJson(p.entries)},"entryValues":${charSeqArrayJson(p.entryValues)}""" +
                ""","currentValue":${stringArrayJson(current)},"default":${stringArrayJson(default)}}"""
        }

        is ListPreference -> {
            """{"type":"ListPreference",$common""" +
                ""","currentValue":${jsonStr(p.currentValue?.toString() ?: "")}""" +
                ""","default":${jsonStr(p.defaultValue?.toString() ?: "")}""" +
                ""","entries":${charSeqArrayJson(p.entries)},"entryValues":${charSeqArrayJson(p.entryValues)}}"""
        }

        is EditTextPreference -> {
            """{"type":"EditTextPreference",$common""" +
                ""","currentValue":${jsonStr(p.currentValue?.toString() ?: "")}""" +
                ""","default":${jsonStr(p.defaultValue?.toString() ?: "")}""" +
                ""","dialogTitle":${dialogTitleJson(p)}""" +
                ""","dialogMessage":${jsonStr(p.dialogMessage?.toString() ?: "")}""" +
                ""","text":${jsonStr(p.text ?: "")}}"""
        }

        // CheckBox 与 Switch 在 WebUI 是两个组件，取值方式相同。
        is CheckBoxPreference -> twoStateJson("CheckBoxPreference", p, common)
        is SwitchPreferenceCompat -> twoStateJson("SwitchPreference", p, common)
        else -> throw IllegalStateException("unsupported preference type: ${p.javaClass.name}")
    }
}

private fun twoStateJson(type: String, p: TwoStatePreference, common: String): String =
    """{"type":"$type",$common""" +
        ""","currentValue":${p.currentValue == true},"default":${p.defaultValue == true}}"""

/**
 * `Preference.getCurrentValue` / `getDefaultValueType` 都要求默认值非空（后者直接
 * `defaultValue.getClass()`），而扩展并不保证每一项都设了默认值 —— 不补的话整页设置 500。
 */
private fun ensureDefault(p: Preference) {
    if (p.defaultValue != null) return
    p.defaultValue = when (p) {
        is MultiSelectListPreference -> HashSet<String>()
        is TwoStatePreference -> false
        else -> ""
    }
}

/**
 * 跑扩展自己的 `setupPreferenceScreen` 得到偏好屏；源没有设置界面时返回 null。
 *
 * 失败不吞：扩展把设置项建不起来（缺类、NPE）是它自己的问题，吞掉只会让前端拿到一个
 * 空白设置页，看不出是坏了还是本来就没设置。
 */
fun buildSourcePreferencesScreen(src: LoadedSource, context: Context?): PreferenceScreen? {
    if (!src.isConfigurable) return null
    val screen = PreferenceScreen(context)
    // `addPreference` 会把这份存储传给每一项，项的 getCurrentValue / saveNewValue 都靠它。
    screen.sharedPreferences = sourceSharedPreferences(src)
    callMethod(src.instance, "setupPreferenceScreen", screen)
    return screen
}

/**
 * 扩展读凭据用的那份 `SharedPreferences`。
 *
 * 优先取扩展自己的 `getSourcePreferences()` —— 业务代码（`headersBuilder` 里取账号密码）
 * 读的就是它，宿主另开一份会变成「设置填了、请求仍说未登录」。扩展用的是打包进去的老 lib
 * 时这个方法是环境给的默认实现，取不到（或抛）就退回宿主按源 id 建的那份。
 */
private fun sourceSharedPreferences(src: LoadedSource): SharedPreferences =
    try {
        callMethod(src.instance, "getSourcePreferences") as? SharedPreferences
    } catch (t: Throwable) {
        null
    } ?: sourcePreferences("source_${src.id}")

/** 偏好屏的 JSON；源没有设置界面时返回 null。 */
fun sourcePreferencesJson(src: LoadedSource, context: Context?): String? =
    buildSourcePreferencesScreen(src, context)?.let { preferenceScreenToJson(it) }

/**
 * 按位置写回一个值，返回写回后的完整 JSON。
 *
 * [value] 的解释方式由该项的默认值类型决定：`String` 原样、`Boolean` 走 `toBoolean`、
 * `Set<String>` 是 JSON 数组。
 */
fun writeSourcePreference(src: LoadedSource, context: Context?, position: Int, value: String): String? {
    val screen = buildSourcePreferencesScreen(src, context) ?: return null
    val pref = preferenceList(screen).getOrNull(position)
        ?: throw IllegalArgumentException("no preference at position $position")
    if (pref.isEnabled) {
        ensureDefault(pref)
        val newValue: Any = when (pref.defaultValueType) {
            "String" -> value
            "Boolean" -> value.toBoolean()
            "Set<String>" -> Json.parseToJsonElement(value).jsonArray.map { it.jsonPrimitive.content }.toSet()
            else -> throw IllegalArgumentException("unsupported preference type ${pref.defaultValueType}")
        }
        // 顺序不能反：先落盘，监听器里读到的才是新值。
        pref.saveNewValue(newValue)
        pref.callChangeListener(newValue)
    }
    return preferenceScreenToJson(screen)
}

/**
 * 对话框标题：扩展没自己设就用设置项标题。
 *
 * 前端把 `dialogTitle` 直接当 `<DialogTitle>` 渲染，空值就是一条空白标题栏 ——
 * 哔咔的「用户名」「密码」正是这种（扩展只给了标题）。Android 的
 * `PreferenceDialogFragmentCompat` 也是这么补的（对话框标题为空时退回设置项标题）。
 */
private fun dialogTitleJson(p: androidx.preference.DialogPreference): String {
    val own = p.dialogTitle?.toString()
    val title = p.title?.toString()
    return jsonStr(if (own.isNullOrEmpty()) (title ?: "") else own)
}

private fun charSeqArrayJson(a: Array<out CharSequence>?): String =    "[" + (a?.joinToString(",") { jsonStr(it.toString()) } ?: "") + "]"

private fun stringArrayJson(a: Collection<String>): String =
    "[" + a.joinToString(",") { jsonStr(it) } + "]"
