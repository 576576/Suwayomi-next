//! dex→jar 转换：dex2jar 标准链 + 一处分配类型修复。
//!
//! dex2jar 的 `NewTransformer` 把 `local = new T` 与紧随的构造调用合并时，取的是
//! **被调构造器的 owner** 而不是 `new` 的类型。R8 对没有构造器的类发的是
//! `new-instance v0, Le;` + `invoke-direct {v0}, Ljava/lang/Object;-><init>()V`
//! （dex 只要求调用某个祖先的构造器），合并结果于是变成 `new java/lang/Object`；
//! JVM 校验器接着在 `putfield e.a` 处发现栈上是 Object 而字段属 `e`，抛 VerifyError
//! 拒绝整个类。这里在 IR 进入标准链之前把 `<init>` 的 owner 改回分配类型，dex2jar
//! 就能生成 `new e; invokespecial e.<init>`。
//!
//! 配套条件：该类在 JAR 里得有匹配的构造器，缺则由 [BytecodeFixer] 按父类构造器补齐
//! （否则退化成 NoSuchMethodError）。
//!
//! 用 [ExDex2Asm] 自己驱动转换而不是 `Dex2jar` 门面，是因为要覆写 `optimize`；
//! 门面里那条链写在匿名类中，无法插入预处理。`Dex2Asm.optimize` 本身就是完整标准链。
package sandbox

import com.googlecode.d2j.Method
import com.googlecode.d2j.dex.ClassVisitorFactory
import com.googlecode.d2j.dex.DexExceptionHandler
import com.googlecode.d2j.dex.ExDex2Asm
import com.googlecode.d2j.dex.LambadaNameSafeClassAdapter
import com.googlecode.d2j.node.DexFileNode
import com.googlecode.d2j.reader.BaseDexFileReader
import com.googlecode.d2j.reader.DexFileReader
import com.googlecode.dex2jar.ir.IrMethod
import com.googlecode.dex2jar.ir.StmtTraveler
import com.googlecode.dex2jar.ir.expr.InvokeExpr
import com.googlecode.dex2jar.ir.expr.Local
import com.googlecode.dex2jar.ir.expr.NewExpr
import com.googlecode.dex2jar.ir.expr.Value
import com.googlecode.dex2jar.ir.stmt.Stmt
import com.googlecode.dex2jar.tools.Constants
import org.objectweb.asm.ClassVisitor
import org.objectweb.asm.ClassWriter
import java.nio.file.FileSystems
import java.nio.file.Files
import java.nio.file.Path

internal object DexTranslator {

    /** 与 `Dex2jar` 门面同口径：跳过调试信息、不清理名称、重算栈映射帧。 */
    private const val READER_CONFIG = DexFileReader.SKIP_DEBUG or
        DexFileReader.DONT_SANITIZE_NAMES or
        DexFileReader.COMPUTE_FRAMES

    /** 把 [reader] 里的 dex 转成 [out]（一个 jar/zip）。 */
    fun translate(reader: BaseDexFileReader, out: Path, handler: DexExceptionHandler) {
        val fileNode = DexFileNode()
        reader.accept(fileNode, READER_CONFIG or DexFileReader.IGNORE_READ_EXCEPTION)

        val parents = HashMap<String, String>()
        for (clz in fileNode.clzs) {
            val superName = clz.superClass ?: continue
            parents[toInternal(clz.className)] = toInternal(superName)
        }

        val parentDir = out.parent
        if (parentDir != null) Files.createDirectories(parentDir)
        Files.deleteIfExists(out)
        FileSystems.newFileSystem(out, mapOf("create" to "true")).use { fs ->
            val root = fs.getPath("/")
            val cvf = object : ClassVisitorFactory {
                override fun create(name: String): ClassVisitor {
                    val cw = DexClassWriter(parents)
                    val safe = LambadaNameSafeClassAdapter(cw, true)
                    return object : ClassVisitor(Constants.ASM_VERSION, safe) {
                        override fun visitEnd() {
                            super.visitEnd()
                            val data = try {
                                cw.toByteArray()
                            } catch (t: Throwable) {
                                System.err.println("sandbox: ASM failed to build ${safe.className}: $t")
                                handler.handleFileException(t as? Exception ?: Exception(t))
                                return
                            }
                            try {
                                val target = root.resolve(safe.className + ".class")
                                Files.createDirectories(target.parent)
                                Files.write(target, data)
                            } catch (e: Exception) {
                                e.printStackTrace(System.err)
                            }
                        }
                    }
                }
            }
            object : ExDex2Asm(handler) {
                override fun optimize(irMethod: IrMethod) {
                    repairAllocationTypes(irMethod)
                    super.optimize(irMethod)
                }
            }.convertDex(fileNode, cvf)
        }
    }

    /**
     * 把「分配类型为 T 的实例上调用的 `<init>`」的 owner 从祖先改回 T。
     *
     * 调用点与 `new` 是同一个对象，改 owner 不改变语义（T 的构造器等价于 `super(...)`），
     * 但能让下游 merge 出正确的分配类型。
     */
    private fun repairAllocationTypes(irMethod: IrMethod) {
        val allocated = HashMap<Local, String>()
        for (stmt in irMethod.stmts) {
            if (stmt.st != Stmt.ST.ASSIGN || stmt.op1.vt != Value.VT.LOCAL) continue
            val local = stmt.op1 as Local
            val rhs = stmt.op2
            when {
                rhs.vt == Value.VT.NEW -> allocated[local] = (rhs as NewExpr).type
                // 非 `new` 的写入让这个局部重新变成未知类型（SSA 下罕见，但别漏）
                rhs.vt != Value.VT.LOCAL -> allocated.remove(local)
            }
        }
        if (allocated.isEmpty()) return

        object : StmtTraveler() {
            override fun travel(op: Value?): Value? {
                if (op?.vt == Value.VT.INVOKE_SPECIAL) {
                    val ie = op as InvokeExpr
                    if (ie.name == "<init>") {
                        val receiver = ie.ops.firstOrNull()
                        if (receiver != null && receiver.vt == Value.VT.LOCAL) {
                            val type = allocated[receiver as Local]
                            if (type != null && type != ie.owner) {
                                ie.method = Method(type, ie.method.name, ie.method.proto)
                            }
                        }
                    }
                }
                return super.travel(op)
            }
        }.travel(irMethod)
    }

    /** `Lfoo/Bar;` → `foo/Bar`。 */
    private fun toInternal(descriptor: String): String =
        if (descriptor.length > 2 && descriptor[0] == 'L' && descriptor.endsWith(";")) {
            descriptor.substring(1, descriptor.length - 1)
        } else {
            descriptor
        }

    /**
     * 重算栈映射帧时 ASM 要问「两个类型的公共父类」。扩展自己的类不在宿主类路径上，
     * ASM 默认实现加载不到而抛异常，所以先用 dex 内的类层次回答；剩下的（kotlin/
     * okhttp 等宿主类）交给默认实现，它失败时兜底 `java/lang/Object`——保守但合法。
     */
    private class DexClassWriter(private val parents: Map<String, String>) :
        ClassWriter(ClassWriter.COMPUTE_FRAMES) {

        override fun getCommonSuperClass(t1: String, t2: String): String {
            if (t1 == t2) return t1
            val supertypesOfT1 = HashSet<String>()
            supertypesOfT1.add(t1)
            var a = t1
            while (true) {
                a = parents[a] ?: break
                supertypesOfT1.add(a)
            }
            var b = t2
            while (true) {
                if (supertypesOfT1.contains(b)) return b
                b = parents[b] ?: break
            }
            return try {
                super.getCommonSuperClass(t1, t2)
            } catch (t: Throwable) {
                "java/lang/Object"
            }
        }
    }
}
