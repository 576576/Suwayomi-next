package sandbox

/**
 * Runtime helper referenced by [BytecodeFixer] when it repairs the dex2jar
 * defect that turns a custom interceptor into `new java/lang/Object` passed
 * to `OkHttpClient$Builder.addInterceptor/addNetworkInterceptor`.
 *
 * The rewrite substitutes the broken `new Object; dup; <init>` sequence with
 * a load of this no-op interceptor (same instruction length, so the original
 * StackMapTable stays valid). The original interceptor's behaviour is not
 * recoverable from the jar — a transparent pass-through is the safe stand-in.
 */
object BytecodeCompat {
    @JvmField
    val NOOP_INTERCEPTOR: okhttp3.Interceptor = okhttp3.Interceptor { chain -> chain.proceed(chain.request()) }

    /**
     * Stand-in for a broken `new java/lang/Object` that dex2jar emitted where
     * an interface-typed argument was expected (e.g. a lambda for
     * `joinToString(transform = …)`). Null is a legal value for every
     * interface parameter, and Kotlin's collection helpers degrade to
     * `toString()` when the transform is null.
     */
    @JvmField
    val NULL: Any? = null
}
