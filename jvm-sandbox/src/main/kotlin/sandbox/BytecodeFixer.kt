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
                    val mv = super.visitMethod(access, name, descriptor, signature, exceptions)
                    if (name == "<clinit>") {
                        return ClinitFixer(mv, hasDefaultCtor)
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
