package ai.muonium.muongit

/// SHA-1 implementation using java.security.MessageDigest for hardware-accelerated hashing.
/// Parity: libgit2 uses SHA-1 for object IDs (see src/util/hash/sha1/)

import java.security.MessageDigest

private val PROTOTYPE_SHA1 = MessageDigest.getInstance("SHA-1")

class SHA1 {
    internal val digest: MessageDigest = PROTOTYPE_SHA1.clone() as MessageDigest

    fun update(data: ByteArray) {
        digest.update(data)
    }

    fun update(data: ByteArray, offset: Int, len: Int) {
        digest.update(data, offset, len)
    }

    fun update(string: String) {
        update(string.encodeToByteArray())
    }

    fun finalize(): ByteArray {
        return digest.digest()
    }

    companion object {
        fun hash(data: ByteArray): ByteArray {
            val d = PROTOTYPE_SHA1.clone() as MessageDigest
            return d.digest(data)
        }

        fun hash(string: String): ByteArray = hash(string.encodeToByteArray())
    }
}

// Pre-computed type name bytes with trailing space for git object headers
private val TYPE_NAME_BLOB = "blob ".toByteArray()
private val TYPE_NAME_TREE = "tree ".toByteArray()
private val TYPE_NAME_COMMIT = "commit ".toByteArray()
private val TYPE_NAME_TAG = "tag ".toByteArray()

/** Build git object header ("type size\0") into a pre-allocated buffer, returns valid length */
internal fun buildObjectHeaderInto(type: ObjectType, size: Int, buf: ByteArray): Int {
    val typeBytes = when (type) {
        ObjectType.COMMIT -> TYPE_NAME_COMMIT
        ObjectType.TREE -> TYPE_NAME_TREE
        ObjectType.BLOB -> TYPE_NAME_BLOB
        ObjectType.TAG -> TYPE_NAME_TAG
    }
    typeBytes.copyInto(buf)
    var pos = typeBytes.size
    if (size == 0) {
        buf[pos++] = '0'.code.toByte()
    } else {
        val start = pos
        var v = size
        while (v > 0) {
            buf[pos++] = ('0'.code + v % 10).toByte()
            v /= 10
        }
        var lo = start; var hi = pos - 1
        while (lo < hi) {
            val tmp = buf[lo]; buf[lo] = buf[hi]; buf[hi] = tmp; lo++; hi--
        }
    }
    buf[pos++] = 0 // null terminator
    return pos
}

/** Build git object header ("type size\0") as byte array */
internal fun buildObjectHeader(type: ObjectType, size: Int): ByteArray {
    val buf = ByteArray(20)
    val len = buildObjectHeaderInto(type, size, buf)
    return buf.copyOf(len)
}

// OID SHA-1 extensions
fun OID.Companion.hashObject(type: ObjectType, data: ByteArray): OID {
    val headerBuf = ByteArray(20)
    val headerLen = buildObjectHeaderInto(type, data.size, headerBuf)
    val sha = SHA1()
    sha.update(headerBuf, 0, headerLen)
    sha.update(data)
    return OID(sha.finalize())
}

val OID.Companion.SHA1_LENGTH get() = 20
val OID.Companion.SHA1_HEX_LENGTH get() = 40
val OID.isZero get() = raw.all { it == 0.toByte() }
val OID.Companion.ZERO get() = OID(ByteArray(20))
