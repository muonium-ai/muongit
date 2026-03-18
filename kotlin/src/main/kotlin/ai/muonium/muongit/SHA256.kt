package ai.muonium.muongit

/// Pure Kotlin SHA-256 implementation
/// Parity: libgit2 EXPERIMENTAL_SHA256 uses SHA-256 for object IDs

@OptIn(ExperimentalUnsignedTypes::class)
class SHA256Hash {
    private var h0: UInt = 0x6a09e667u
    private var h1: UInt = 0xbb67ae85u
    private var h2: UInt = 0x3c6ef372u
    private var h3: UInt = 0xa54ff53au
    private var h4: UInt = 0x510e527fu
    private var h5: UInt = 0x9b05688cu
    private var h6: UInt = 0x1f83d9abu
    private var h7: UInt = 0x5be0cd19u

    private val buffer = mutableListOf<Byte>()
    private var totalLength: Long = 0L

    fun update(data: ByteArray) {
        buffer.addAll(data.toList())
        totalLength += data.size

        while (buffer.size >= 64) {
            val block = buffer.subList(0, 64).toByteArray()
            processBlock(block)
            repeat(64) { buffer.removeFirst() }
        }
    }

    fun update(string: String) {
        update(string.encodeToByteArray())
    }

    fun finalize(): ByteArray {
        val padded = buffer.toMutableList()
        padded.add(0x80.toByte())

        while (padded.size % 64 != 56) {
            padded.add(0x00.toByte())
        }

        val bitLength = totalLength * 8
        for (i in 7 downTo 0) {
            padded.add(((bitLength shr (i * 8)) and 0xFF).toByte())
        }

        var offset = 0
        while (offset < padded.size) {
            val block = padded.subList(offset, offset + 64).toByteArray()
            processBlock(block)
            offset += 64
        }

        val digest = ByteArray(32)
        for ((i, h) in listOf(h0, h1, h2, h3, h4, h5, h6, h7).withIndex()) {
            digest[i * 4]     = ((h shr 24) and 0xFFu).toByte()
            digest[i * 4 + 1] = ((h shr 16) and 0xFFu).toByte()
            digest[i * 4 + 2] = ((h shr 8) and 0xFFu).toByte()
            digest[i * 4 + 3] = (h and 0xFFu).toByte()
        }
        return digest
    }

    private fun processBlock(block: ByteArray) {
        val w = UIntArray(64)

        for (i in 0 until 16) {
            w[i] = ((block[i * 4].toUInt() and 0xFFu) shl 24) or
                   ((block[i * 4 + 1].toUInt() and 0xFFu) shl 16) or
                   ((block[i * 4 + 2].toUInt() and 0xFFu) shl 8) or
                   (block[i * 4 + 3].toUInt() and 0xFFu)
        }

        for (i in 16 until 64) {
            val s0 = w[i - 15].rotateRight(7) xor w[i - 15].rotateRight(18) xor (w[i - 15] shr 3)
            val s1 = w[i - 2].rotateRight(17) xor w[i - 2].rotateRight(19) xor (w[i - 2] shr 10)
            w[i] = w[i - 16] + s0 + w[i - 7] + s1
        }

        var a = h0; var b = h1; var c = h2; var d = h3
        var e = h4; var f = h5; var g = h6; var h = h7

        for (i in 0 until 64) {
            val s1 = e.rotateRight(6) xor e.rotateRight(11) xor e.rotateRight(25)
            val ch = (e and f) xor (e.inv() and g)
            val temp1 = h + s1 + ch + K[i] + w[i]
            val s0 = a.rotateRight(2) xor a.rotateRight(13) xor a.rotateRight(22)
            val maj = (a and b) xor (a and c) xor (b and c)
            val temp2 = s0 + maj

            h = g
            g = f
            f = e
            e = d + temp1
            d = c
            c = b
            b = a
            a = temp1 + temp2
        }

        h0 += a; h1 += b; h2 += c; h3 += d
        h4 += e; h5 += f; h6 += g; h7 += h
    }

    companion object {
        private val K = uintArrayOf(
            0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u,
            0x3956c25bu, 0x59f111f1u, 0x923f82a4u, 0xab1c5ed5u,
            0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u,
            0x72be5d74u, 0x80deb1feu, 0x9bdc06a7u, 0xc19bf174u,
            0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu,
            0x2de92c6fu, 0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau,
            0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u,
            0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u,
            0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu, 0x53380d13u,
            0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u,
            0xa2bfe8a1u, 0xa81a664bu, 0xc24b8b70u, 0xc76c51a3u,
            0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u,
            0x19a4c116u, 0x1e376c08u, 0x2748774cu, 0x34b0bcb5u,
            0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
            0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u,
            0x90befffau, 0xa4506cebu, 0xbef9a3f7u, 0xc67178f2u,
        )

        fun hash(data: ByteArray): ByteArray {
            val sha = SHA256Hash()
            sha.update(data)
            return sha.finalize()
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
    val typeName = when (type) {
        ObjectType.COMMIT -> "commit"
        ObjectType.TREE -> "tree"
        ObjectType.BLOB -> "blob"
        ObjectType.TAG -> "tag"
    }

    val header = "$typeName ${data.size}\u0000"
    val sha = SHA256Hash()
    sha.update(header.encodeToByteArray())
    sha.update(data)
    return OID(sha.finalize())
}

val OID.Companion.SHA256_LENGTH get() = 32
val OID.Companion.SHA256_HEX_LENGTH get() = 64
val OID.Companion.ZERO_SHA256 get() = OID(ByteArray(32))
