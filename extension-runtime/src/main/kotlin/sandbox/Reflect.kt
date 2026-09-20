//! 反射工具：驱动扩展 source 的公共底座。
//!
//! 这些函数两端共用（桌面 jvm-sandbox 与 Android extension-host）：
//! 只依赖 JDK 反射 + kotlinx-coroutines + rxjava，不碰 Android/桌面专有 API。
//! 之所以全走反射，是因为扩展编译时链接的 `eu.kanade.tachiyomi.*` 版本各不相同，
//! 宿主不能编译期绑定它们的字段/方法名。

package sandbox

/** 一个已加载的扩展 source，全程通过反射驱动。 */
class LoadedSource(
    val id: Long,
    val name: String,
    val lang: String,
    val extensionId: Long,
    val instance: Any,
    val sourceCls: Class<*>,
    val smangaCls: Class<*>,
    val schapterCls: Class<*>,
    val pageCls: Class<*>,
    val mangasPageCls: Class<*>,
    /** 实现 `ConfigurableSource`：有设置界面可填（账号、服务器地址等）。 */
    val isConfigurable: Boolean = false,
    /** 提供"最近更新"列表；读的是源自己的 `supportsLatest`。 */
    val supportsLatest: Boolean = false,
)

/**
 * 实例是否实现名为 [interfaceName] 的接口（沿父接口递归）。
 *
 * 按名字而不是编译期类型判断：扩展的类加载器是 child-first 的，若某个扩展自带一份
 * `eu.kanade.tachiyomi.**`，`is ConfigurableSource` 会因为两份 Class 不同而恒为 false。
 */
fun implementsInterface(instance: Any, interfaceName: String): Boolean =
    collectInterfaceNames(instance.javaClass).contains(interfaceName)

private fun collectInterfaceNames(cls: Class<*>?, acc: MutableSet<String> = LinkedHashSet()): Set<String> {
    var cur = cls
    while (cur != null && acc.add(cur.name)) {
        cur.interfaces.forEach { collectInterfaceNames(it, acc) }
        cur = cur.superclass
    }
    return acc
}

/** 框架自带的源基类所在包：Source / CatalogueSource / online.HttpSource。 */
private const val FRAMEWORK_SOURCE_PACKAGE = "eu.kanade.tachiyomi.source"

/**
 * 该方法是不是**扩展自己**实现的，而不是框架基类给的默认实现。
 *
 * 判据是"沿父类链最先声明它的那个类落在哪个包"：框架基类保留原名（`eu.kanade.tachiyomi.source.**`），
 * 扩展自己的类名会被 R8 改写成 `keiyoushi.source.Generated` / `a0` 之类。
 */
fun isExtensionOwnMethod(obj: Any, name: String, paramCount: Int): Boolean {
    var cls: Class<*>? = obj.javaClass
    while (cls != null) {
        if (cls.declaredMethods.any { it.name == name && it.parameterCount == paramCount }) {
            val pkg = cls.name.substringBeforeLast('.', "")
            return pkg != FRAMEWORK_SOURCE_PACKAGE && !pkg.startsWith("$FRAMEWORK_SOURCE_PACKAGE.")
        }
        cls = cls.superclass
    }
    return false
}

/** 读源的 `supportsLatest`；读不到（老 lib 没有这个 getter）按不支持算。 */
fun readSupportsLatest(instance: Any): Boolean =
    try {
        callGetter(instance, "getSupportsLatest") as? Boolean ?: false
    } catch (t: Throwable) {
        false
    }

fun findMethod(cls: Class<*>, name: String, vararg paramTypes: Class<*>): java.lang.reflect.Method? =
    try {
        cls.getMethod(name, *paramTypes)
    } catch (e: NoSuchMethodException) {
        // `getMethod` requires exact parameter types. Extension methods often
        // declare interfaces (SManga) while we hold the impl class (SMangaImpl),
        // so fall back to name+arity and validate assignability.
        cls.methods.firstOrNull { m ->
            m.name == name &&
                m.parameterCount == paramTypes.size &&
                m.parameterTypes.indices.all { i -> m.parameterTypes[i].isAssignableFrom(paramTypes[i]) }
        }
    }

fun callGetter(obj: Any, getterName: String): Any? {
    val m = findMethod(obj.javaClass, getterName) ?: return null
    return m.invoke(obj)
}

fun callMethod(obj: Any, name: String, vararg args: Any?): Any? {
    val types = args.map { primitiveOf(it?.javaClass ?: Any::class.java) }.toTypedArray()
    // try exact match first, then walk up to superclass methods
    var cls: Class<*>? = obj.javaClass
    while (cls != null) {
        val m = findMethod(cls, name, *types)
        if (m != null) {
            return try {
                m.invoke(obj, *args)
            } catch (e: java.lang.reflect.InvocationTargetException) {
                // unwrap nested InvocationTargetException chains and log the
                // full cause stack so the underlying failure is visible in
                // sandbox.log instead of a bare "InvocationTargetException".
                var cause: Throwable? = e
                while (cause is java.lang.reflect.InvocationTargetException && cause.cause != null && cause.cause !== cause) {
                    cause = cause.cause
                }
                cause?.printStackTrace()
                throw RuntimeException("$name failed: ${cause?.message ?: e}", cause)
            }
        }
        cls = cls.superclass
    }
    // interface methods may be declared on a parent interface — try by name only
    val any = obj.javaClass.methods.firstOrNull { it.name == name && it.parameterCount == args.size }
        ?: throw RuntimeException("no method $name(${args.size} args) on ${obj.javaClass.name}")
    return any.invoke(obj, *args)
}

/** Bridge continuation that turns a Kotlin suspend call into a blocking one. */
class BridgeContinuation<T>(
    private val ctx: kotlin.coroutines.CoroutineContext = kotlin.coroutines.EmptyCoroutineContext,
) : kotlin.coroutines.Continuation<T> {
    private val deferred = kotlinx.coroutines.CompletableDeferred<T>()
    override val context: kotlin.coroutines.CoroutineContext get() = ctx
    override fun resumeWith(result: Result<T>) {
        result.fold(
            onSuccess = { deferred.complete(it) },
            onFailure = { deferred.completeExceptionally(it) },
        )
    }
    fun awaitBlocking(): T = kotlinx.coroutines.runBlocking { deferred.await() }
}

/**
 * Calls a Kotlin suspend function `name(args..., Continuation)` reflectively and
 * blocks until it completes. Used for new keiyoushi extensions (lib 2.x) whose
 * sources implement the suspend `getPopularManga`/`getSearchManga`/… instead of
 * the legacy rx.Observable `fetch*` methods.
 */
fun callSuspendMethod(obj: Any, name: String, vararg args: Any?): Any? {
    val types = args.map { primitiveOf(it?.javaClass ?: Any::class.java) }.toTypedArray() +
        arrayOf(kotlin.coroutines.Continuation::class.java)
    val m = findMethod(obj.javaClass, name, *types)
        ?: throw RuntimeException("no suspend method $name(${args.size}+1 args) on ${obj.javaClass.name}")
    val cont = BridgeContinuation<Any?>()
    val result = try {
        val allArgs: Array<Any?> = arrayOf(*args, cont)
        m.invoke(obj, *allArgs)
    } catch (e: java.lang.reflect.InvocationTargetException) {
        var cause: Throwable? = e
        while (cause is java.lang.reflect.InvocationTargetException && cause.cause != null && cause.cause !== cause) {
            cause = cause.cause
        }
        cause?.printStackTrace()
        throw RuntimeException("$name failed: ${cause?.message ?: e}", cause)
    }
    return if (result === kotlin.coroutines.intrinsics.COROUTINE_SUSPENDED) {
        cont.awaitBlocking()
    } else {
        result
    }
}

/** Maps a wrapper type to its primitive, if any (JVM methods use `int` etc.). */
private fun primitiveOf(c: Class<*>): Class<*> = when (c) {
    java.lang.Integer::class.java -> java.lang.Integer.TYPE
    java.lang.Long::class.java -> java.lang.Long.TYPE
    java.lang.Float::class.java -> java.lang.Float.TYPE
    java.lang.Double::class.java -> java.lang.Double.TYPE
    java.lang.Boolean::class.java -> java.lang.Boolean.TYPE
    else -> c
}

/** Reflectively builds a tachiyomi SManga/SChapter/Page instance from a JSON-ish map. */
fun buildModel(cls: Class<*>, fields: Map<String, Any?>): Any {
    val ctor = try {
        cls.getDeclaredConstructor()
    } catch (e: NoSuchMethodException) {
        // data class with all-default params still exposes a no-arg ctor in Kotlin 1.9+
        val c = cls.declaredConstructors.firstOrNull { it.parameterCount == 0 }
            ?: cls.declaredConstructors.minByOrNull { it.parameterCount }!!
        c.isAccessible = true
        return c.newInstance()
    }
    ctor.isAccessible = true
    val obj = ctor.newInstance()
    for ((k, v) in fields) {
        setField(obj, k, v)
    }
    return obj
}

fun setField(obj: Any, name: String, value: Any?) {
    val cls = obj.javaClass
    // walk up the class hierarchy
    var c: Class<*>? = cls
    while (c != null) {
        val f = try {
            c.getDeclaredField(name)
        } catch (e: NoSuchFieldException) {
            null
        }
        if (f != null) {
            f.isAccessible = true
            val converted = convert(f.type, value)
            f.set(obj, converted)
            return
        }
        c = c.superclass
    }
    // Kotlin data classes compile fields as private + getter/setter — try the setter
    val setterName = "set" + name.replaceFirstChar { it.uppercase() }
    val setter = findMethod(cls, setterName) ?: return
    setter.invoke(obj, convert(setter.parameterTypes[0], value))
}

fun convert(target: Class<*>, value: Any?): Any? {
    if (value == null) return null
    return when {
        target.isInstance(value) -> value
        target == java.lang.Long::class.java || target == java.lang.Long.TYPE -> (value as? Number)?.toLong() ?: 0L
        target == java.lang.Integer::class.java || target == java.lang.Integer.TYPE -> (value as? Number)?.toInt() ?: 0
        target == java.lang.Float::class.java || target == java.lang.Float.TYPE -> (value as? Number)?.toFloat() ?: 0f
        target == java.lang.Double::class.java || target == java.lang.Double.TYPE -> (value as? Number)?.toDouble() ?: 0.0
        target == java.lang.Boolean::class.java || target == java.lang.Boolean.TYPE -> (value as? Boolean) ?: false
        else -> value.toString()
    }
}

/** Kotlin data-class getter field read (e.g. `getUrl()`), walking the hierarchy. */
fun readField(obj: Any?, name: String): Any? {
    if (obj == null) return null
    val getter = findMethod(obj.javaClass, "get" + name.replaceFirstChar { it.uppercase() })
        ?: findMethod(obj.javaClass, "is" + name.replaceFirstChar { it.uppercase() })
    if (getter != null) return try { getter.invoke(obj) } catch (e: Exception) { null }
    var c: Class<*>? = obj.javaClass
    while (c != null) {
        val f = try {
            c.getDeclaredField(name)
        } catch (e: NoSuchFieldException) {
            null
        }
        if (f != null) {
            f.isAccessible = true
            return try { f.get(obj) } catch (e: Exception) { null }
        }
        c = c.superclass
    }
    return null
}
