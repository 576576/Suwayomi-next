package app.cash.quickjs;

import java.io.Closeable;
import org.mozilla.javascript.Context;
import org.mozilla.javascript.NativeArray;
import org.mozilla.javascript.Scriptable;
import org.mozilla.javascript.ScriptableObject;
import org.mozilla.javascript.Undefined;
import org.mozilla.javascript.Wrapper;

/**
 * quickjs-android 的 {@code app.cash.quickjs.QuickJs} 的取代实现，跑在 Rhino 上。
 *
 * 扩展的 dex 是按 quickjs-android 的签名链接到这里的（那个库带 native .so，桌面上没有），
 * 所以公开方法与它逐一对应：create / evaluate / compile / execute / set / close。
 *
 * 不在字段里持有 {@link Context}：Rhino 的 Context 绑线程（`Context.enter()`），持有它会让
 * 另一个线程上的 evaluate 抛「from a different thread」。每次调用自己 enter/exit，只留
 * `initStandardObjects()` 产出的 scope。
 *
 * {@code setOptimizationLevel(-1)} 走解释模式：这里执行的是扩展里几十行的签名/解密片段，
 * 生成字节码的收益抵不上类加载开销。
 */
public final class QuickJs implements Closeable {
    private Scriptable scope;

    public static QuickJs create() {
        return new QuickJs();
    }

    public QuickJs() {
        Context cx = Context.enter();
        try {
            this.scope = initScope(cx);
        } finally {
            Context.exit();
        }
    }

    private static Scriptable initScope(Context cx) {
        cx.setOptimizationLevel(-1);
        cx.setLanguageVersion(Context.VERSION_ES6);
        return cx.initStandardObjects();
    }

    public Object evaluate(String script, String ignoredFileName) {
        return this.evaluate(script);
    }

    public Object evaluate(String script) {
        Context cx = Context.enter();
        try {
            initScope(cx);
            return translateType(cx.evaluateString(scope, script, "<eval>", 1, null));
        } catch (Exception exception) {
            throw new QuickJsException(exception.getMessage(), exception);
        } finally {
            Context.exit();
        }
    }

    /**
     * 把 JS 值翻成扩展期待的 Java 类型：数字按 int / long / double 收敛，数组按首元素定型。
     * 空数组给 {@code int[0]}，与 quickjs-android 的返回一致。
     *
     * 宿主对象（JS 里调过 Java 方法拿到的返回值）是 `NativeJavaObject` 这种包装，要拆回原对象 ——
     * 扩展拿到的会是包装类，`as String` / `as Int` 全失败。
     */
    private static Object translateType(Object obj) {
        if (obj == null || Undefined.isUndefined(obj)) {
            return obj;
        } else if (obj instanceof Boolean) {
            return obj;
        } else if (obj instanceof NativeArray) {
            NativeArray array = (NativeArray) obj;
            int size = (int) array.getLength();
            if (size == 0) {
                return new int[0];
            }
            Object first = array.get(0, array);
            if (first instanceof Boolean) {
                boolean[] out = new boolean[size];
                for (int i = 0; i < size; i++) {
                    out[i] = Context.toBoolean(array.get(i, array));
                }
                return out;
            } else if (first instanceof Number) {
                return translateNumberArray(array, size);
            } else if (first instanceof CharSequence) {
                String[] out = new String[size];
                for (int i = 0; i < size; i++) {
                    out[i] = Context.toString(array.get(i, array));
                }
                return out;
            }
            Object[] out = new Object[size];
            for (int i = 0; i < size; i++) {
                out[i] = translateType(array.get(i, array));
            }
            return out;
        } else if (obj instanceof Wrapper) {
            Object unwrapped = ((Wrapper) obj).unwrap();
            return unwrapped == obj ? obj : translateType(unwrapped);
        } else if (obj instanceof Number) {
            return translateNumber((Number) obj);
        } else if (obj instanceof CharSequence) {
            return obj.toString();
        }
        return obj;
    }

    private static Object translateNumberArray(NativeArray array, int size) {
        double[] values = new double[size];
        boolean allInt = true;
        boolean allLong = true;
        for (int i = 0; i < size; i++) {
            double d = Context.toNumber(array.get(i, array));
            values[i] = d;
            if (Double.isNaN(d) || d != Math.rint(d) || d < Integer.MIN_VALUE || d > Integer.MAX_VALUE) {
                allInt = false;
            }
            if (Double.isNaN(d) || d != Math.rint(d) || d < Long.MIN_VALUE || d > Long.MAX_VALUE) {
                allLong = false;
            }
        }
        if (allInt) {
            int[] out = new int[size];
            for (int i = 0; i < size; i++) {
                out[i] = (int) values[i];
            }
            return out;
        }
        if (allLong) {
            long[] out = new long[size];
            for (int i = 0; i < size; i++) {
                out[i] = (long) values[i];
            }
            return out;
        }
        return values;
    }

    private static Object translateNumber(Number number) {
        double d = number.doubleValue();
        if (!Double.isNaN(d) && !Double.isInfinite(d) && d == Math.rint(d)) {
            if (d >= Integer.MIN_VALUE && d <= Integer.MAX_VALUE) {
                return (int) d;
            }
            if (d >= Long.MIN_VALUE && d <= Long.MAX_VALUE) {
                return (long) d;
            }
        }
        return d;
    }

    /**
     * 只回源码字节：quickjs-android 这里返回的是 native 编译产物，扩展接着交给 {@link #execute}，
     * 两步合起来等价于一次 evaluate。
     */
    public byte[] compile(String sourceCode, String ignoredFileName) {
        return sourceCode.getBytes();
    }

    public Object execute(byte[] bytecode) {
        return this.evaluate(new String(bytecode));
    }

    public <T> void set(String name, Class<T> ignoredType, T object) {
        Context cx = Context.enter();
        try {
            ScriptableObject.putProperty(scope, name, Context.javaToJS(object, scope));
        } finally {
            Context.exit();
        }
    }

    @Override
    public void close() {
        this.scope = null;
    }
}
