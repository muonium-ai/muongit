package ai.muonium.muongit

/// SHA-256 implementation using java.security.MessageDigest for hardware-accelerated hashing.
/// Parity: libgit2 EXPERIMENTAL_SHA256 uses SHA-256 for object IDs

import java.security.MessageDigest

private val PROTOTYPE_SHA256 = MessageDigest.getInstance("SHA-256")

@OptIn(ExperimentalUnsignedTypes::class)
class SHA256Hash {
    internal val digest: MessageDigest = PROTOTYPE_SHA256.clone() as MessageDigest

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
            val d = PROTOTYPE_SHA256.clone() as MessageDigest
            return d.digest(data)
        }

        fun hash(string: String): ByteArray = hash(string.encodeToByteArray())
    }
}

/** Hash algorithm selection (matching libgit2 EXPERIMENTAL_SHA256) */
enum class HashAlgorithm(val digestLength: Int) {
    SHA1(20),
    SHA256(32);

    /** Hex string length */
    val hexLength: Int get() = digestLength * 2
}

// OID SHA-256 extensions
fun OID.Companion.hashObjectSHA256(type: ObjectType, data: ByteArray): OID {
    val headerBuf = ByteArray(20)
    val headerLen = buildObjectHeaderInto(type, data.size, headerBuf)
    val sha = SHA256Hash()
    sha.update(headerBuf, 0, headerLen)
    sha.update(data)
    return OID(sha.finalize())
}

val OID.Companion.SHA256_LENGTH get() = 32
val OID.Companion.SHA256_HEX_LENGTH get() = 64
val OID.Companion.ZERO_SHA256 get() = OID(ByteArray(32))
