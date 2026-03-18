package ai.muonium.muongit

import java.io.ByteArrayOutputStream
import java.io.RandomAccessFile
import java.util.zip.Deflater
import java.util.zip.Inflater

private const val OBJ_COMMIT: Int = 1
private const val OBJ_TREE: Int = 2
private const val OBJ_BLOB: Int = 3
private const val OBJ_TAG: Int = 4
private const val OBJ_OFS_DELTA: Int = 6
private const val OBJ_REF_DELTA: Int = 7

/** Result of reading a pack object */
data class PackObject(
    val objType: ObjectType,
    val data: ByteArray,
) {
    override fun equals(other: Any?): Boolean =
        other is PackObject && objType == other.objType && data.contentEquals(other.data)
    override fun hashCode(): Int = objType.hashCode() * 31 + data.contentHashCode()
}

/** Read an object from a pack file at the given offset. */
fun readPackObject(packPath: String, offset: Long, index: PackIndex): PackObject {
    val raf = RandomAccessFile(packPath, "r")
    raf.use { return readObjectAt(it, offset, index) }
}

private fun readObjectAt(raf: RandomAccessFile, offset: Long, index: PackIndex): PackObject {
    raf.seek(offset)

    val (typeNum, _) = readTypeAndSize(raf)

    return when (typeNum) {
        OBJ_COMMIT, OBJ_TREE, OBJ_BLOB, OBJ_TAG -> {
            val objType = packTypeToObjectType(typeNum)
            val data = decompressStream(raf)
            PackObject(objType, data)
        }
        OBJ_OFS_DELTA -> {
            val baseOffset = readOfsDeltaOffset(raf)
            val deltaData = decompressStream(raf)
            val base = readObjectAt(raf, offset - baseOffset, index)
            val result = applyDelta(base.data, deltaData)
            PackObject(base.objType, result)
        }
        OBJ_REF_DELTA -> {
            val oidBytes = ByteArray(20)
            raf.readFully(oidBytes)
            val baseOid = OID(oidBytes)
            val deltaData = decompressStream(raf)

            val basePackOffset = index.find(baseOid)
                ?: throw MuonGitException.NotFound("base object ${baseOid.hex} not found in pack index")
            val base = readObjectAt(raf, basePackOffset, index)
            val result = applyDelta(base.data, deltaData)
            PackObject(base.objType, result)
        }
        else -> throw MuonGitException.InvalidObject("unknown pack object type $typeNum")
    }
}

private fun readTypeAndSize(raf: RandomAccessFile): Pair<Int, Long> {
    val c = raf.read()
    if (c < 0) throw MuonGitException.InvalidObject("unexpected EOF in pack")

    val typeNum = (c shr 4) and 0x07
    var size = (c and 0x0F).toLong()
    var shift = 4

    if (c and 0x80 != 0) {
        while (true) {
            val b = raf.read()
            if (b < 0) throw MuonGitException.InvalidObject("unexpected EOF in pack header")
            size = size or ((b and 0x7F).toLong() shl shift)
            shift += 7
            if (b and 0x80 == 0) break
        }
    }

    return Pair(typeNum, size)
}

private fun readOfsDeltaOffset(raf: RandomAccessFile): Long {
    var c = raf.read()
    if (c < 0) throw MuonGitException.InvalidObject("unexpected EOF in ofs delta")
    var offset = (c and 0x7F).toLong()

    while (c and 0x80 != 0) {
        offset += 1
        c = raf.read()
        if (c < 0) throw MuonGitException.InvalidObject("unexpected EOF in ofs delta offset")
        offset = (offset shl 7) or (c and 0x7F).toLong()
    }

    return offset
}

private fun decompressStream(raf: RandomAccessFile): ByteArray {
    val currentPos = raf.filePointer
    val remaining = (raf.length() - currentPos).toInt()
    val compressed = ByteArray(remaining)
    raf.readFully(compressed)

    val inflater = Inflater()
    inflater.setInput(compressed)
    val output = ByteArrayOutputStream()
    val buf = ByteArray(4096)
    while (!inflater.finished()) {
        val n = inflater.inflate(buf)
        if (n == 0 && inflater.needsInput()) break
        output.write(buf, 0, n)
    }

    // Seek back to where compressed data ended
    val consumed = inflater.bytesRead
    inflater.end()
    raf.seek(currentPos + consumed)

    return output.toByteArray()
}

private fun packTypeToObjectType(t: Int): ObjectType = when (t) {
    OBJ_COMMIT -> ObjectType.COMMIT
    OBJ_TREE -> ObjectType.TREE
    OBJ_BLOB -> ObjectType.BLOB
    OBJ_TAG -> ObjectType.TAG
    else -> throw MuonGitException.InvalidObject("invalid object type $t")
}

/** Apply a git delta to a base object. */
fun applyDelta(base: ByteArray, delta: ByteArray): ByteArray {
    var pos = 0

    val (_, srcConsumed) = readDeltaSize(delta, pos)
    pos += srcConsumed

    val (tgtSize, tgtConsumed) = readDeltaSize(delta, pos)
    pos += tgtConsumed

    val result = ByteArrayOutputStream(tgtSize.toInt())

    while (pos < delta.size) {
        val cmd = delta[pos].toInt() and 0xFF
        pos++

        if (cmd and 0x80 != 0) {
            // Copy from base
            var copyOffset = 0
            var copySize = 0

            if (cmd and 0x01 != 0) { copyOffset = copyOffset or (delta[pos].toInt() and 0xFF); pos++ }
            if (cmd and 0x02 != 0) { copyOffset = copyOffset or ((delta[pos].toInt() and 0xFF) shl 8); pos++ }
            if (cmd and 0x04 != 0) { copyOffset = copyOffset or ((delta[pos].toInt() and 0xFF) shl 16); pos++ }
            if (cmd and 0x08 != 0) { copyOffset = copyOffset or ((delta[pos].toInt() and 0xFF) shl 24); pos++ }

            if (cmd and 0x10 != 0) { copySize = copySize or (delta[pos].toInt() and 0xFF); pos++ }
            if (cmd and 0x20 != 0) { copySize = copySize or ((delta[pos].toInt() and 0xFF) shl 8); pos++ }
            if (cmd and 0x40 != 0) { copySize = copySize or ((delta[pos].toInt() and 0xFF) shl 16); pos++ }

            if (copySize == 0) copySize = 0x10000

            if (copyOffset + copySize > base.size) {
                throw MuonGitException.InvalidObject("delta copy out of bounds")
            }
            result.write(base, copyOffset, copySize)
        } else if (cmd > 0) {
            // Insert new data
            if (pos + cmd > delta.size) {
                throw MuonGitException.InvalidObject("delta insert out of bounds")
            }
            result.write(delta, pos, cmd)
            pos += cmd
        } else {
            throw MuonGitException.InvalidObject("invalid delta opcode 0")
        }
    }

    val bytes = result.toByteArray()
    if (bytes.size.toLong() != tgtSize) {
        throw MuonGitException.InvalidObject("delta result size mismatch")
    }

    return bytes
}

private fun readDeltaSize(data: ByteArray, start: Int): Pair<Long, Int> {
    var pos = start
    var size = 0L
    var shift = 0

    while (pos < data.size) {
        val c = data[pos].toInt() and 0xFF
        pos++
        size = size or ((c and 0x7F).toLong() shl shift)
        shift += 7
        if (c and 0x80 == 0) break
    }

    return Pair(size, pos - start)
}

/** Build a minimal pack file for testing. */
internal fun buildTestPack(objects: List<Pair<ObjectType, ByteArray>>): ByteArray {
    val buf = ByteArrayOutputStream()

    // Header
    buf.write("PACK".toByteArray())
    buf.write(packObjWriteU32(2))
    buf.write(packObjWriteU32(objects.size))

    for ((objType, data) in objects) {
        val typeNum = when (objType) {
            ObjectType.COMMIT -> OBJ_COMMIT
            ObjectType.TREE -> OBJ_TREE
            ObjectType.BLOB -> OBJ_BLOB
            ObjectType.TAG -> OBJ_TAG
        }

        val size = data.size.toLong()
        val headerBytes = ByteArrayOutputStream()
        val first = ((typeNum shl 4) or (size.toInt() and 0x0F))
        var remaining = size shr 4

        if (remaining > 0) {
            headerBytes.write(first or 0x80)
            while (remaining > 0) {
                val byte = (remaining and 0x7F).toInt()
                remaining = remaining shr 7
                if (remaining > 0) {
                    headerBytes.write(byte or 0x80)
                } else {
                    headerBytes.write(byte)
                }
            }
        } else {
            headerBytes.write(first)
        }

        buf.write(headerBytes.toByteArray())

        // Compress data
        val deflater = Deflater()
        deflater.setInput(data)
        deflater.finish()
        val compBuf = ByteArray(data.size + 64)
        val compLen = deflater.deflate(compBuf)
        deflater.end()
        buf.write(compBuf, 0, compLen)
    }

    // Pack checksum
    val content = buf.toByteArray()
    val checksum = SHA1.hash(content)
    buf.write(checksum)

    return buf.toByteArray()
}

private fun packObjWriteU32(value: Int): ByteArray = byteArrayOf(
    ((value shr 24) and 0xFF).toByte(),
    ((value shr 16) and 0xFF).toByte(),
    ((value shr 8) and 0xFF).toByte(),
    (value and 0xFF).toByte(),
)
