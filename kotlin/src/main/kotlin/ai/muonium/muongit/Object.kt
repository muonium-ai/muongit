package ai.muonium.muongit

import java.io.File

/** A generic git object loaded from the object database. */
class GitObject(
    val oid: OID,
    val objectType: ObjectType,
    val data: ByteArray,
) {
    val size: Int get() = data.size

    fun asBlob(): Blob {
        if (objectType != ObjectType.BLOB) {
            throw MuonGitException.InvalidObject("expected blob, got $objectType")
        }
        return Blob(oid = oid, data = data)
    }

    fun asCommit(): Commit {
        if (objectType != ObjectType.COMMIT) {
            throw MuonGitException.InvalidObject("expected commit, got $objectType")
        }
        return parseCommit(oid, data)
    }

    fun asTree(): Tree {
        if (objectType != ObjectType.TREE) {
            throw MuonGitException.InvalidObject("expected tree, got $objectType")
        }
        return parseTree(oid, data)
    }

    fun asTag(): Tag {
        if (objectType != ObjectType.TAG) {
            throw MuonGitException.InvalidObject("expected tag, got $objectType")
        }
        return parseTag(oid, data)
    }

    fun peel(gitDir: File): GitObject {
        var current = this
        val seen = mutableSetOf(current.oid)

        while (current.objectType == ObjectType.TAG) {
            val tag = current.asTag()
            if (!seen.add(tag.targetId)) {
                throw MuonGitException.InvalidObject("tag peel cycle detected")
            }
            current = readObject(gitDir, tag.targetId)
        }

        return current
    }

    override fun equals(other: Any?): Boolean =
        other is GitObject &&
            oid == other.oid &&
            objectType == other.objectType &&
            data.contentEquals(other.data)

    override fun hashCode(): Int =
        ((oid.hashCode() * 31) + objectType.hashCode()) * 31 + data.contentHashCode()
}

/** Read a generic object by OID from loose or packed storage. */
fun readObject(gitDir: File, oid: OID): GitObject {
    return try {
        val (objType, data) = readLooseObject(gitDir, oid)
        GitObject(oid = oid, objectType = objType, data = data)
    } catch (_: MuonGitException.NotFound) {
        readPackedObject(gitDir, oid)
    }
}

fun Repository.readObject(oid: OID): GitObject = readObject(gitDir, oid)

private fun readPackedObject(gitDir: File, oid: OID): GitObject {
    val packDir = File(gitDir, "objects/pack")
    val entries = packDir.listFiles()?.sortedBy { it.name }
        ?: throw MuonGitException.NotFound("object not found: ${oid.hex}")

    for (idxFile in entries) {
        if (idxFile.extension != "idx") {
            continue
        }
        val idx = readPackIndex(idxFile.path)
        val offset = idx.find(oid) ?: continue
        val packFile = File(idxFile.parentFile, "${idxFile.nameWithoutExtension}.pack")
        val packObject = readPackObject(packFile.path, offset, idx)
        return GitObject(oid = oid, objectType = packObject.objType, data = packObject.data)
    }

    throw MuonGitException.NotFound("object not found: ${oid.hex}")
}
