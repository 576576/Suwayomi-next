//! Bytecode repair for R8-obfuscated extension classes.
//!
//! keiyoushi builds are processed by R8, which sometimes emits a broken
//! `<clinit>` for companion objects: `new java/lang/Object` followed (possibly
//! after a store/load pair) by `putstatic <owner>.Companion:L<actual-type>;`.
//! Android's dex verifier tolerates it; the JVM verifier rejects it with a
//! `VerifyError`. This transformer rewrites the offending sequence to
//! construct the real type (or null when the type has no constructor and is
//! never dereferenced). It also synthesizes missing no-arg constructors.
package sandbox

import org.objectweb.asm.ClassReader
import org.objectweb.asm.ClassVisitor
import org.objectweb.asm.ClassWriter
import org.objectweb.asm.MethodVisitor
import org.objectweb.asm.Opcodes

object BytecodeFixer {

    /**
     * COMPUTE_FRAMES is deliberately NOT used: dex2jar emits StackMapTables
     * whose declared types are sometimes wider than a re-derived dataflow
     * (e.g. a merge point typed `int` in the table but `Object` per the
     * instructions). The JVM verifier trusts the table at basic-block entry,
     * so the original frames pass — but recomputing them surfaces
     * `VerifyError: Bad type on operand stack`. The interceptor rewrite
     * below is length-preserving precisely so the original frames stay valid.
     */
    /** Rewrites broken `<clinit>` sequences in [bytes]. */
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
     * Repairs a dex2jar conversion defect seen in keiyoushi builds: a custom
     * interceptor that R8 kept (e.g. `IgnoreGzipInterceptor`) sometimes comes
     * out of dex2jar as a bare `new java/lang/Object` passed to
     * `OkHttpClient$Builder.addInterceptor/addNetworkInterceptor`. Android's
     * dex verifier tolerates it (the original type was a normal object), but
     * on the JVM it becomes a `ClassCastException: Object cannot be cast to
     * okhttp3.Interceptor` the first time the client runs a request — which
     * bricks popular/search entirely for the affected source.
     *
     * The original interceptor's behaviour is unrecoverable from the jar, so
     * the broken `new Object; dup; <init>` sequence is replaced with a load
     * of [BytecodeCompat.NOOP_INTERCEPTOR] (transparent pass-through). The
     * substitution is length-preserving (GETSTATIC 3B ≈ NEW 3B, DUP/INVOKESPECIAL
     * → NOP), so code offsets and the original StackMapTable stay valid and
     * the class still verifies.
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
            // Second dex2jar defect: `new java/lang/Object` where an interface
            // argument is expected, e.g. the transform lambda of
            // `joinToString$default(...)` — other arguments sit between the
            // broken `new` and the call, so the whole expression is buffered.
            // Null is a valid value for any interface parameter, so substitute
            // NULL (length-preserving) and replay the remaining args unchanged.
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
