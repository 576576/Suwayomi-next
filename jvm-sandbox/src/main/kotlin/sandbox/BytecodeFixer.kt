//! R8 混淆扩展字节码修复：keiyoushi 系构建经 R8 后偶发坏 `<clinit>`
//! （companion 对象：`new java/lang/Object` + `putstatic …Companion:L<实际类型>`）。
//! dex 校验器容忍、JVM 校验器抛 VerifyError——本转换把该序列改写为构造真实类型
//! （类型无构造器且不被解引用时置 null），并合成缺失的无参构造器。
package sandbox

import org.objectweb.asm.ClassReader
import org.objectweb.asm.ClassVisitor
import org.objectweb.asm.ClassWriter
import org.objectweb.asm.MethodVisitor
import org.objectweb.asm.Opcodes

object BytecodeFixer {

    /**
     * 刻意不用 COMPUTE_FRAMES：dex2jar 的 StackMapTable 声明的类型可能比重推
     * 数据流更宽（表里 int、指令流实际 Object），JVM 校验器在基本块入口信任
     * 原表——重算反而报 VerifyError。下面的改写是等长的，正是为保原帧有效。
     */
    /** 修复 [bytes] 中坏的 `<clinit>` 序列。 */
    fun fix(bytes: ByteArray, hasDefaultCtor: (String) -> Boolean = { true }): ByteArray {
        val cr = ClassReader(bytes)
        val cw = ClassWriter(cr, 0)
        var sawInit = false
        cr.accept(
            object : ClassVisitor(Opcodes.ASM9, cw) {
                override fun visitMethod(
                    access: Int,
                    name: String?,
                    descriptor: String?,
                    signature: String?,
                    exceptions: Array<out String>?,
                ): MethodVisitor {
                    if (name == "<init>") sawInit = true
                    var mv = super.visitMethod(access, name, descriptor, signature, exceptions)
                    // dex2jar sometimes mangles a custom interceptor's `new`
                    // into `new java/lang/Object` (see [InterceptorAddFixer]) —
                    // repair in every method, not just <clinit>.
                    mv = InterceptorAddFixer(mv)
                    if (name == "<clinit>") {
                        mv = ClinitFixer(mv, hasDefaultCtor)
                    }
                    return mv
                }
            },
            0,
        )
        // R8 sometimes strips the constructor from synthetic companion classes.
        // Code that dereferences the companion needs a real instance.
        if (!sawInit && cr.access and Opcodes.ACC_INTERFACE == 0) {
            val mv = cw.visitMethod(Opcodes.ACC_PUBLIC, "<init>", "()V", null, null)
            mv?.visitCode()
            mv?.visitVarInsn(Opcodes.ALOAD, 0)
            mv?.visitMethodInsn(Opcodes.INVOKESPECIAL, "java/lang/Object", "<init>", "()V", false)
            mv?.visitInsn(Opcodes.RETURN)
            mv?.visitMaxs(1, 1)
            mv?.visitEnd()
        }
        return cw.toByteArray()
    }

    private class Pending(val opcode: Int, val type: String?, val owner: String?, val name: String?, val desc: String?)

    /**
     * 修 dex2jar 另一类缺陷：keiyoushi 里被 R8 保留的自定义拦截器（如
     * IgnoreGzipInterceptor）经 dex2jar 后变成裸 `new java/lang/Object` 传给
     * OkHttpClient.Builder.addInterceptor——dex 校验器容忍，JVM 上首次请求即
     * ClassCastException，整个源的热门/搜索瘫痪。原始行为不可恢复，故替换为
     * 透传的 [BytecodeCompat.NOOP_INTERCEPTOR]；替换等长（GETSTATIC≈NEW，
     * DUP/INVOKESPECIAL→NOP），保偏移与 StackMapTable 有效。
     */
    private class InterceptorAddFixer(
        mv: MethodVisitor?,
    ) : MethodVisitor(Opcodes.ASM9, mv) {
        private var buffering = false
        private val buf = mutableListOf<Pending>()

        override fun visitTypeInsn(opcode: Int, type: String?) {
            if (!buffering && opcode == Opcodes.NEW && type == "java/lang/Object") {
                buffering = true
                buf.clear()
                buf.add(Pending(opcode, type, null, null, null))
                return
            }
            if (buffering) {
                if (buf.size > 16) return flushBuf()
                buf.add(Pending(opcode, type, null, null, null))
                return
            }
            super.visitTypeInsn(opcode, type)
        }

        override fun visitMethodInsn(opcode: Int, owner: String?, name: String?, descriptor: String?, isInterface: Boolean) {
            if (!buffering) {
                super.visitMethodInsn(opcode, owner, name, descriptor, isInterface)
                return
            }
            if (opcode == Opcodes.INVOKESPECIAL && owner == "java/lang/Object" && name == "<init>") {
                buf.add(Pending(opcode, null, owner, name, descriptor))
                return
            }
            if (opcode == Opcodes.INVOKEVIRTUAL && (name == "addInterceptor" || name == "addNetworkInterceptor")) {
                val ownerOk = owner?.startsWith("okhttp3/") == true && owner.endsWith("OkHttpClient\$Builder")
                val descOk = descriptor?.contains("Lokhttp3/Interceptor;") == true
                if (ownerOk && descOk && buf[0].opcode == Opcodes.NEW && buf[0].type == "java/lang/Object") {
                    // matched: NEW Object; DUP?; INVOKESPECIAL <init>; add*Interceptor(...)
                    // rewrite to: GETSTATIC NOOP_INTERCEPTOR; NOP...; add*Interceptor(...)
                    emitSubstitution("sandbox/BytecodeCompat", "NOOP_INTERCEPTOR", "Lokhttp3/Interceptor;")
                    super.visitMethodInsn(opcode, owner, name, descriptor, isInterface)
                    return
                }
            }
            // 第二类缺陷：接口形参位置出现 `new java/lang/Object`（如
            // joinToString$default 的 lambda）。参数被缓冲，替换为 NULL
            // （接口形参合法值，等长）并原样重放其余参数。
            val desc = descriptor.orEmpty()
            val params = desc.substringBefore(')').removePrefix("(")
            // lambda-typed interfaces that R8 emits `new Object` for
            val expectsInterface = params.contains("Lkotlin/jvm/functions/") ||
                params.contains("Ljava/util/Comparator;") ||
                params.contains("Ljava/util/function/") ||
                params.contains("Ljava/lang/Runnable;") ||
                params.contains("Ljava/util/concurrent/Callable;")
            if (buf[0].opcode == Opcodes.NEW && buf[0].type == "java/lang/Object" && expectsInterface) {
                emitSubstitution("sandbox/BytecodeCompat", "NULL", "Ljava/lang/Object;")
                super.visitMethodInsn(opcode, owner, name, descriptor, isInterface)
                return
            }
            flushBuf()
            super.visitMethodInsn(opcode, owner, name, descriptor, isInterface)
        }

        /**
         * Replaces the buffered broken `NEW Object` sequence with a static
         * field load of the same length (GETSTATIC ≈ NEW), NOPs for DUP and
         * INVOKESPECIAL, then replays any remaining buffered args — keeping
         * code offsets and the StackMapTable valid.
         */
        private fun emitSubstitution(owner: String, field: String, fieldDesc: String) {
            var first = true
            for (p in buf) {
                when {
                    first -> {
                        mv?.visitFieldInsn(Opcodes.GETSTATIC, owner, field, fieldDesc)
                        first = false
                    }
                    p.opcode == Opcodes.DUP || p.opcode == Opcodes.INVOKESPECIAL -> mv?.visitInsn(Opcodes.NOP)
                    p.opcode == Opcodes.NEW || p.opcode == Opcodes.ANEWARRAY || p.opcode == Opcodes.CHECKCAST || p.opcode == Opcodes.INSTANCEOF ->
                        mv?.visitTypeInsn(p.opcode, p.type)
                    p.opcode == Opcodes.ASTORE || p.opcode == Opcodes.ALOAD || p.opcode == Opcodes.ISTORE || p.opcode == Opcodes.ILOAD ||
                    p.opcode == Opcodes.LSTORE || p.opcode == Opcodes.LLOAD || p.opcode == Opcodes.FSTORE || p.opcode == Opcodes.FLOAD ||
                    p.opcode == Opcodes.DSTORE || p.opcode == Opcodes.DLOAD ->
                        mv?.visitVarInsn(p.opcode, p.desc?.toIntOrNull() ?: 0)
                    p.opcode == Opcodes.BIPUSH || p.opcode == Opcodes.SIPUSH -> mv?.visitIntInsn(p.opcode, p.desc?.toIntOrNull() ?: 0)
                    else -> mv?.visitInsn(p.opcode)
                }
            }
            buffering = false
            buf.clear()
        }

        override fun visitInsn(opcode: Int) {
            if (buffering) {
                if (buf.size > 16) return flushBuf()
                buf.add(Pending(opcode, null, null, null, null))
                return
            }
            super.visitInsn(opcode)
        }

        override fun visitVarInsn(opcode: Int, index: Int) {
            if (buffering) {
                if (buf.size > 16) return flushBuf()
                buf.add(Pending(opcode, null, null, null, index.toString()))
                return
            }
            super.visitVarInsn(opcode, index)
        }

        // Any instruction type we don't pattern-match must break the buffer:
        // emitting it out-of-band would reorder instructions relative to the
        // buffered NEW Object sequence and break verification.
        private fun flushIfBuffering() {
            if (buffering) flushBuf()
        }

        override fun visitIntInsn(opcode: Int, operand: Int) {
            if (buffering) {
                if (buf.size > 16) return flushBuf()
                buf.add(Pending(opcode, null, null, null, operand.toString()))
                return
            }
            super.visitIntInsn(opcode, operand)
        }

        override fun visitLdcInsn(cst: Any?) {
            flushIfBuffering()
            super.visitLdcInsn(cst)
        }

        override fun visitJumpInsn(opcode: Int, label: org.objectweb.asm.Label?) {
            flushIfBuffering()
            super.visitJumpInsn(opcode, label)
        }

        override fun visitFieldInsn(opcode: Int, owner: String?, name: String?, descriptor: String?) {
            flushIfBuffering()
            super.visitFieldInsn(opcode, owner, name, descriptor)
        }

        override fun visitInvokeDynamicInsn(name: String?, descriptor: String?, bsm: org.objectweb.asm.Handle?, vararg bsmArgs: Any?) {
            flushIfBuffering()
            super.visitInvokeDynamicInsn(name, descriptor, bsm, *bsmArgs)
        }

        override fun visitIincInsn(index: Int, increment: Int) {
            flushIfBuffering()
            super.visitIincInsn(index, increment)
        }

        override fun visitTableSwitchInsn(min: Int, max: Int, dflt: org.objectweb.asm.Label?, vararg labels: org.objectweb.asm.Label?) {
            flushIfBuffering()
            super.visitTableSwitchInsn(min, max, dflt, *labels)
        }

        override fun visitLookupSwitchInsn(dflt: org.objectweb.asm.Label?, keys: IntArray?, labels: Array<out org.objectweb.asm.Label>?) {
            flushIfBuffering()
            super.visitLookupSwitchInsn(dflt, keys, labels)
        }

        override fun visitMultiANewArrayInsn(descriptor: String?, numDimensions: Int) {
            flushIfBuffering()
            super.visitMultiANewArrayInsn(descriptor, numDimensions)
        }

        override fun visitEnd() {
            flushIfBuffering()
            super.visitEnd()
        }

        override fun visitFrame(type: Int, numLocal: Int, local: Array<out Any?>?, numStack: Int, stack: Array<out Any?>?) {
            flushBuf()
            super.visitFrame(type, numLocal, local, numStack, stack)
        }

        override fun visitLabel(label: org.objectweb.asm.Label?) {
            if (buffering) flushBuf()
            super.visitLabel(label)
        }

        private fun flushBuf() {
            if (!buffering) return
            buffering = false
            for (p in buf) {
                when (p.opcode) {
                    Opcodes.NEW, Opcodes.ANEWARRAY, Opcodes.CHECKCAST, Opcodes.INSTANCEOF -> mv?.visitTypeInsn(p.opcode, p.type)
                    Opcodes.ASTORE, Opcodes.ALOAD, Opcodes.ISTORE, Opcodes.ILOAD,
                    Opcodes.LSTORE, Opcodes.LLOAD, Opcodes.FSTORE, Opcodes.FLOAD,
                    Opcodes.DSTORE, Opcodes.DLOAD,
                    -> mv?.visitVarInsn(p.opcode, p.desc?.toIntOrNull() ?: 0)
                    Opcodes.BIPUSH, Opcodes.SIPUSH -> mv?.visitIntInsn(p.opcode, p.desc?.toIntOrNull() ?: 0)
                    Opcodes.INVOKEVIRTUAL, Opcodes.INVOKESPECIAL, Opcodes.INVOKESTATIC, Opcodes.INVOKEINTERFACE ->
                        mv?.visitMethodInsn(p.opcode, p.owner, p.name, p.desc, p.opcode == Opcodes.INVOKEINTERFACE)
                    Opcodes.GETSTATIC, Opcodes.PUTSTATIC, Opcodes.GETFIELD, Opcodes.PUTFIELD ->
                        mv?.visitFieldInsn(p.opcode, p.owner, p.name, p.desc)
                    else -> mv?.visitInsn(p.opcode)
                }
            }
            buf.clear()
        }
    }


    /**
     * Buffers instructions after `NEW java/lang/Object` and, when the sequence
     * ends in `PUTSTATIC <field>:L<Target>;` (with optional store/load pairs),
     * rewrites the NEW + INVOKESPECIAL to construct `<Target>`.
     */
    private class ClinitFixer(
        mv: MethodVisitor?,
        private val hasDefaultCtor: (String) -> Boolean,
    ) : MethodVisitor(Opcodes.ASM9, mv) {
        private var buffering = false
        private val buf = mutableListOf<Pending>()

        override fun visitTypeInsn(opcode: Int, type: String?) {
            if (opcode == Opcodes.NEW && type == "java/lang/Object" && !buffering) {
                buffering = true
                buf.clear()
                buf.add(Pending(opcode, type, null, null, null))
                return
            }
            if (buffering) {
                buf.add(Pending(opcode, type, null, null, null))
                return
            }
            super.visitTypeInsn(opcode, type)
        }

        override fun visitInsn(opcode: Int) {
            if (buffering) {
                if (buf.size > 64) return flushBuf()
                buf.add(Pending(opcode, null, null, null, null))
                return
            }
            super.visitInsn(opcode)
        }

        override fun visitVarInsn(opcode: Int, index: Int) {
            if (buffering) {
                if (buf.size > 64) return flushBuf()
                buf.add(Pending(opcode, null, null, null, index.toString()))
                return
            }
            super.visitVarInsn(opcode, index)
        }

        override fun visitMethodInsn(opcode: Int, owner: String?, name: String?, descriptor: String?, isInterface: Boolean) {
            if (buffering) {
                buf.add(Pending(opcode, null, owner, name, descriptor))
                return
            }
            super.visitMethodInsn(opcode, owner, name, descriptor, isInterface)
        }

        override fun visitFieldInsn(opcode: Int, owner: String?, name: String?, descriptor: String?) {
            if (buffering && opcode == Opcodes.PUTSTATIC) {
                ownerName = owner
                fieldName = name
                fieldDesc = descriptor
                val target = descriptor?.removePrefix("L")?.removeSuffix(";")
                if (tryRewrite(target)) {
                    return
                }
            }
            if (buffering) {
                buf.add(Pending(opcode, null, owner, name, descriptor))
                return
            }
            super.visitFieldInsn(opcode, owner, name, descriptor)
        }

        override fun visitFrame(type: Int, numLocal: Int, local: Array<out Any?>?, numStack: Int, stack: Array<out Any?>?) {
            // Frames inside a buffered region would invalidate the rewrite;
            // flush as-is (safety valve).
            flushBuf()
            super.visitFrame(type, numLocal, local, numStack, stack)
        }

        override fun visitLabel(label: org.objectweb.asm.Label?) {
            if (buffering) {
                // labels break the straight-line pattern; flush original
                flushBuf()
            }
            super.visitLabel(label)
        }

        /** Returns true when [buf] matched the broken pattern and was rewritten. */
        private fun tryRewrite(target: String?): Boolean {
            if (target == null || target == "java/lang/Object") return false
            // Expected shape:
            //   [NEW Object]
            //   [DUP]?
            //   [INVOKESPECIAL Object.<init>]
            //   (ASTORE x / ALOAD x)*
            var i = 0
            if (buf.size < 1) return false
            if (buf[0].opcode != Opcodes.NEW) return false
            i = 1
            if (i < buf.size && buf[i].opcode == Opcodes.DUP) i++
            if (i < buf.size && buf[i].opcode == Opcodes.INVOKESPECIAL &&
                buf[i].owner == "java/lang/Object" && buf[i].name == "<init>"
            ) {
                i++
            } else {
                return false
            }
            // optional astore/aload pairs
            while (i < buf.size &&
                (buf[i].opcode == Opcodes.ASTORE || buf[i].opcode == Opcodes.ALOAD)
            ) {
                i++
            }
            if (i != buf.size) return false

            // Rewrite: construct the real target type. The synthesizer below
            // guarantees every non-interface class gets a no-arg <init>, so
            // `new <target>; invokespecial <target>.<init>` always resolves.
            mv?.visitTypeInsn(Opcodes.NEW, target)
            mv?.visitInsn(Opcodes.DUP)
            mv?.visitMethodInsn(Opcodes.INVOKESPECIAL, target, "<init>", "()V", false)
            // replay the store/load tail
            var k = 1
            if (k < buf.size && buf[k].opcode == Opcodes.DUP) k++
            if (k < buf.size && buf[k].opcode == Opcodes.INVOKESPECIAL) k++
            while (k < buf.size) {
                val p = buf[k]
                if (p.opcode == Opcodes.ASTORE || p.opcode == Opcodes.ALOAD) {
                    mv?.visitVarInsn(p.opcode, p.desc?.toIntOrNull() ?: 0)
                }
                k++
            }
            mv?.visitFieldInsn(Opcodes.PUTSTATIC, ownerName, fieldName, fieldDesc)
            buffering = false
            buf.clear()
            return true
        }

        private fun flushBuf() {
            if (!buffering) return
            buffering = false
            for (p in buf) {
                when (p.opcode) {
                    Opcodes.NEW, Opcodes.ANEWARRAY, Opcodes.CHECKCAST, Opcodes.INSTANCEOF -> mv?.visitTypeInsn(p.opcode, p.type)
                    Opcodes.ASTORE, Opcodes.ALOAD, Opcodes.ISTORE, Opcodes.ILOAD,
                    Opcodes.LSTORE, Opcodes.LLOAD, Opcodes.FSTORE, Opcodes.FLOAD,
                    Opcodes.DSTORE, Opcodes.DLOAD,
                    -> mv?.visitVarInsn(p.opcode, p.desc?.toIntOrNull() ?: 0)
                    Opcodes.INVOKEVIRTUAL, Opcodes.INVOKESPECIAL, Opcodes.INVOKESTATIC, Opcodes.INVOKEINTERFACE ->
                        mv?.visitMethodInsn(p.opcode, p.owner, p.name, p.desc, p.opcode == Opcodes.INVOKEINTERFACE)
                    Opcodes.GETSTATIC, Opcodes.PUTSTATIC, Opcodes.GETFIELD, Opcodes.PUTFIELD ->
                        mv?.visitFieldInsn(p.opcode, p.owner, p.name, p.desc)
                    else -> mv?.visitInsn(p.opcode)
                }
            }
            buf.clear()
        }

        // last PUTSTATIC operands captured on the failing path
        private var ownerName: String? = null
        private var fieldName: String? = null
        private var fieldDesc: String? = null
    }
}
