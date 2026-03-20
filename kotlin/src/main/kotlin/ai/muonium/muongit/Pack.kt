package ai.muonium.muongit

import java.io.ByteArrayOutputStream
import java.io.File
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

data class IndexedPack(
    val packName: String,
    val packPath: String,
    val indexPath: String,
    val objectCount: Int,
)

private data class RawPackEntry(
    val offset: Long,
    val kind: RawPackEntryKind,
)

private sealed interface RawPackEntryKind {
    data class Base(val objType: ObjectType, val data: ByteArray) : RawPackEntryKind
    data class OfsDelta(val baseOffset: Long, val delta: ByteArray) : RawPackEntryKind
    data class RefDelta(val baseOID: OID, val delta: ByteArray) : RawPackEntryKind
}

private data class ResolvedPackEntry(
    val offset: Long,
    val oid: OID,
    val objType: ObjectType,
    val data: ByteArray,
)

/** Read an object from a pack file at the given offset. */
fun readPackObject(packPath: String, offset: Long, index: PackIndex): PackObject {
    val raf = RandomAccessFile(packPath, "r")
    raf.use { return readObjectAt(it, offset, index) }
}

fun indexPackToODB(gitDir: File, packBytes: ByteArray): IndexedPack {
    val (entries, packChecksum) = parsePackEntries(packBytes)
    val resolved = resolvePackEntries(entries, gitDir)
    val sorted = resolved.sortedWith { a, b -> compareBytes(a.oid.raw, b.oid.raw) }

    val idxBytes = buildPackIndexWithChecksums(
        oids = sorted.map { it.oid },
        crcs = IntArray(sorted.size),
        offsets = sorted.map { it.offset }.toLongArray(),
        packChecksum = packChecksum
    )

    val packDir = File(gitDir, "objects/pack")
    packDir.mkdirs()

    val packHex = hexBytes(packChecksum)
    val packName = "pack-$packHex"
    val packFile = File(packDir, "$packName.pack")
    val idxFile = File(packDir, "$packName.idx")

    writeIfMissing(packFile, packBytes)
    writeIfMissing(idxFile, idxBytes)

    return IndexedPack(
        packName = packName,
        packPath = packFile.path,
        indexPath = idxFile.path,
        objectCount = resolved.size,
    )
}

fun buildPackFromOIDs(gitDir: File, roots: List<OID>, exclude: List<OID>): ByteArray {
    val visited = mutableSetOf<OID>()
    val excluded = exclude.toHashSet()
    val ordered = mutableListOf<OID>()

    for (root in roots) {
        collectReachableObjects(gitDir, root, excluded, visited, ordered)
    }

    val buf = ByteArrayOutputStream()
    buf.write("PACK".toByteArray())
    buf.write(packObjWriteU32(2))
    buf.write(packObjWriteU32(ordered.size))

    for (oid in ordered) {
        val obj = readObject(gitDir, oid)
        appendPackObject(buf, obj.objectType, obj.data)
    }

    val content = buf.toByteArray()
    val checksum = SHA1.hash(content)
    buf.write(checksum)
    return buf.toByteArray()
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

private fun appendPackObject(buf: ByteArrayOutputStream, objType: ObjectType, data: ByteArray) {
    val typeNum = when (objType) {
        ObjectType.COMMIT -> OBJ_COMMIT
        ObjectType.TREE -> OBJ_TREE
        ObjectType.BLOB -> OBJ_BLOB
        ObjectType.TAG -> OBJ_TAG
    }

    var size = data.size.toLong()
    var first = ((typeNum shl 4) or (size.toInt() and 0x0F))
    size = size shr 4
    if (size == 0L) {
        buf.write(first)
    } else {
        first = first or 0x80
        buf.write(first)
        while (size > 0) {
            var byte = (size and 0x7F).toInt()
            size = size shr 7
            if (size > 0) {
                byte = byte or 0x80
            }
            buf.write(byte)
        }
    }

    val deflater = Deflater()
    deflater.setInput(data)
    deflater.finish()
    val compressed = ByteArrayOutputStream()
    val out = ByteArray(4096)
    while (!deflater.finished()) {
        val n = deflater.deflate(out)
        compressed.write(out, 0, n)
    }
    deflater.end()
    buf.write(compressed.toByteArray())
}

private fun writeIfMissing(path: File, data: ByteArray) {
    if (path.exists()) {
        return
    }
    path.writeBytes(data)
}

private fun hexBytes(bytes: ByteArray): String =
    bytes.joinToString("") { "%02x".format(it) }

private fun parsePackEntries(data: ByteArray): Pair<List<RawPackEntry>, ByteArray> {
    if (data.size < 32) {
        throw MuonGitException.InvalidObject("pack file too short")
    }
    if (!data.copyOfRange(0, 4).contentEquals("PACK".toByteArray())) {
        throw MuonGitException.InvalidObject("bad pack magic")
    }

    val version = readPackU32(data, 4)
    if (version != 2 && version != 3) {
        throw MuonGitException.InvalidObject("unsupported pack version $version")
    }

    val objectCount = readPackU32(data, 8)
    val contentLen = data.size - 20
    val expectedChecksum = SHA1.hash(data.copyOfRange(0, contentLen))
    val packChecksum = data.copyOfRange(contentLen, data.size)
    if (!packChecksum.contentEquals(expectedChecksum)) {
        throw MuonGitException.InvalidObject("pack checksum mismatch")
    }

    var cursor = 12
    val entries = mutableListOf<RawPackEntry>()
    repeat(objectCount) {
        if (cursor >= contentLen) {
            throw MuonGitException.InvalidObject("pack truncated before advertised object count")
        }

        val offset = cursor.toLong()
        val (typeNum, _, headerLen) = parseTypeAndSize(data, cursor, contentLen)
        cursor += headerLen

        val kind = when (typeNum) {
            OBJ_COMMIT, OBJ_TREE, OBJ_BLOB, OBJ_TAG -> {
                val objType = packTypeToObjectType(typeNum)
                val (inflated, consumed) = inflateZlibStream(data, cursor, contentLen)
                cursor += consumed
                RawPackEntryKind.Base(objType, inflated)
            }
            OBJ_OFS_DELTA -> {
                val (distance, consumedHeader) = parseOfsDeltaDistance(data, cursor, contentLen)
                cursor += consumedHeader
                val (delta, consumed) = inflateZlibStream(data, cursor, contentLen)
                cursor += consumed
                if (offset < distance) {
                    throw MuonGitException.InvalidObject("invalid ofs-delta base")
                }
                RawPackEntryKind.OfsDelta(offset - distance, delta)
            }
            OBJ_REF_DELTA -> {
                if (cursor + 20 > contentLen) {
                    throw MuonGitException.InvalidObject("truncated ref-delta base OID")
                }
                val baseOID = OID(data.copyOfRange(cursor, cursor + 20))
                cursor += 20
                val (delta, consumed) = inflateZlibStream(data, cursor, contentLen)
                cursor += consumed
                RawPackEntryKind.RefDelta(baseOID, delta)
            }
            else -> throw MuonGitException.InvalidObject("unknown pack object type $typeNum")
        }

        entries.add(RawPackEntry(offset, kind))
    }

    if (cursor != contentLen) {
        throw MuonGitException.InvalidObject("pack contains trailing bytes after object stream")
    }

    return Pair(entries, packChecksum)
}

private fun resolvePackEntries(entries: List<RawPackEntry>, gitDir: File?): List<ResolvedPackEntry> {
    val resolved = arrayOfNulls<ResolvedPackEntry>(entries.size)
    val offsetToIndex = entries.mapIndexed { index, entry -> entry.offset to index }.toMap()
    val oidToIndex = mutableMapOf<OID, Int>()
    var remaining = entries.size

    while (remaining > 0) {
        var progressed = false

        for ((index, entry) in entries.withIndex()) {
            if (resolved[index] != null) {
                continue
            }

            val resolvedEntry = when (val kind = entry.kind) {
                is RawPackEntryKind.Base -> {
                    val oid = OID.hashObject(kind.objType, kind.data)
                    ResolvedPackEntry(entry.offset, oid, kind.objType, kind.data)
                }
                is RawPackEntryKind.OfsDelta -> {
                    val base = offsetToIndex[kind.baseOffset]?.let { resolved[it] }
                        ?: continue
                    val data = applyDelta(base.data, kind.delta)
                    val oid = OID.hashObject(base.objType, data)
                    ResolvedPackEntry(entry.offset, oid, base.objType, data)
                }
                is RawPackEntryKind.RefDelta -> {
                    val resolvedBase = oidToIndex[kind.baseOID]?.let { resolved[it] }
                    val base = when {
                        resolvedBase != null -> Pair(resolvedBase.objType, resolvedBase.data)
                        gitDir != null -> runCatching { readObject(gitDir, kind.baseOID) }
                            .getOrNull()
                            ?.let { obj -> Pair(obj.objectType, obj.data) }
                        else -> null
                    } ?: continue
                    val data = applyDelta(base.second, kind.delta)
                    val oid = OID.hashObject(base.first, data)
                    ResolvedPackEntry(entry.offset, oid, base.first, data)
                }
            }

            resolved[index] = resolvedEntry
            oidToIndex[resolvedEntry.oid] = index
            remaining--
            progressed = true
        }

        if (!progressed) {
            throw MuonGitException.InvalidObject("could not resolve all pack deltas")
        }
    }

    return resolved.filterNotNull()
}

private fun parseTypeAndSize(data: ByteArray, start: Int, limit: Int): Triple<Int, Long, Int> {
    if (start >= limit) {
        throw MuonGitException.InvalidObject("unexpected EOF in pack object header")
    }
    val first = data[start].toInt() and 0xFF
    val typeNum = (first shr 4) and 0x07
    var size = (first and 0x0F).toLong()
    var shift = 4
    var consumed = 1
    var current = first

    while (current and 0x80 != 0) {
        if (start + consumed >= limit) {
            throw MuonGitException.InvalidObject("truncated pack object header")
        }
        current = data[start + consumed].toInt() and 0xFF
        size = size or ((current and 0x7F).toLong() shl shift)
        shift += 7
        consumed++
    }

    return Triple(typeNum, size, consumed)
}

private fun parseOfsDeltaDistance(data: ByteArray, start: Int, limit: Int): Pair<Long, Int> {
    if (start >= limit) {
        throw MuonGitException.InvalidObject("unexpected EOF in ofs-delta")
    }
    var consumed = 1
    var c = data[start].toInt() and 0xFF
    var offset = (c and 0x7F).toLong()

    while (c and 0x80 != 0) {
        if (start + consumed >= limit) {
            throw MuonGitException.InvalidObject("truncated ofs-delta offset")
        }
        offset += 1
        c = data[start + consumed].toInt() and 0xFF
        offset = (offset shl 7) or (c and 0x7F).toLong()
        consumed++
    }

    return Pair(offset, consumed)
}

private fun inflateZlibStream(data: ByteArray, start: Int, end: Int): Pair<ByteArray, Int> {
    val inflater = Inflater()
    val input = data.copyOfRange(start, end)
    inflater.setInput(input)
    val output = ByteArrayOutputStream()
    val buf = ByteArray(4096)

    try {
        while (true) {
            val n = inflater.inflate(buf)
            if (n > 0) {
                output.write(buf, 0, n)
            }
            if (inflater.finished()) {
                break
            }
            if (n == 0 && inflater.needsInput()) {
                throw MuonGitException.InvalidObject("failed to inflate pack stream")
            }
        }
        return Pair(output.toByteArray(), inflater.bytesRead.toInt())
    } finally {
        inflater.end()
    }
}

private fun collectReachableObjects(
    gitDir: File,
    oid: OID,
    exclude: Set<OID>,
    visited: MutableSet<OID>,
    ordered: MutableList<OID>,
) {
    if (exclude.contains(oid) || !visited.add(oid)) {
        return
    }

    val obj = readObject(gitDir, oid)
    when (obj.objectType) {
        ObjectType.COMMIT -> {
            val commit = obj.asCommit()
            collectReachableObjects(gitDir, commit.treeId, exclude, visited, ordered)
            for (parent in commit.parentIds) {
                collectReachableObjects(gitDir, parent, exclude, visited, ordered)
            }
        }
        ObjectType.TREE -> {
            val tree = obj.asTree()
            for (entry in tree.entries) {
                collectReachableObjects(gitDir, entry.oid, exclude, visited, ordered)
            }
        }
        ObjectType.TAG -> {
            val tag = obj.asTag()
            collectReachableObjects(gitDir, tag.targetId, exclude, visited, ordered)
        }
        ObjectType.BLOB -> Unit
    }

    ordered.add(oid)
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
