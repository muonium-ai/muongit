package ai.muonium.muongit

import java.io.File

/** The kind of change for a diff entry */
enum class DiffStatus {
    ADDED,
    DELETED,
    MODIFIED,
}

/** A single diff delta between two trees */
data class DiffDelta(
    val status: DiffStatus,
    val oldEntry: TreeEntry?,
    val newEntry: TreeEntry?,
    val path: String,
)

/** Compute the diff between two trees.
 *  Both entry lists should be sorted by name (as git trees are). */
fun diffTrees(oldEntries: List<TreeEntry>, newEntries: List<TreeEntry>): List<DiffDelta> {
    val deltas = mutableListOf<DiffDelta>()
    var oi = 0
    var ni = 0

    while (oi < oldEntries.size && ni < newEntries.size) {
        val old = oldEntries[oi]
        val new = newEntries[ni]

        when {
            old.name < new.name -> {
                deltas.add(DiffDelta(DiffStatus.DELETED, old, null, old.name))
                oi++
            }
            old.name > new.name -> {
                deltas.add(DiffDelta(DiffStatus.ADDED, null, new, new.name))
                ni++
            }
            else -> {
                if (old.oid != new.oid || old.mode != new.mode) {
                    deltas.add(DiffDelta(DiffStatus.MODIFIED, old, new, old.name))
                }
                oi++
                ni++
            }
        }
    }

    while (oi < oldEntries.size) {
        val old = oldEntries[oi]
        deltas.add(DiffDelta(DiffStatus.DELETED, old, null, old.name))
        oi++
    }

    while (ni < newEntries.size) {
        val new = newEntries[ni]
        deltas.add(DiffDelta(DiffStatus.ADDED, null, new, new.name))
        ni++
    }

    return deltas
}

/** Compute the diff between the index (staging area) and the working directory.
 *  Returns deltas for modified, deleted, and new (untracked) files. */
fun diffIndexToWorkdir(gitDir: File, workdir: File): List<DiffDelta> {
    val index = readIndex(gitDir)
    val deltas = mutableListOf<DiffDelta>()

    val indexedPaths = index.entries.map { it.path }.toSet()

    // Check each index entry against the working directory
    for (entry in index.entries) {
        val file = File(workdir, entry.path)
        if (!file.exists()) {
            deltas.add(DiffDelta(
                DiffStatus.DELETED,
                indexEntryToTreeEntry(entry),
                null,
                entry.path
            ))
        } else {
            val fileSize = file.length().toInt()
            var modified = fileSize != entry.fileSize
            if (!modified) {
                val content = file.readBytes()
                val oid = OID.hashObject(ObjectType.BLOB, content)
                modified = oid != entry.oid
            }

            if (modified) {
                val content = file.readBytes()
                val workdirOid = OID.hashObject(ObjectType.BLOB, content)
                val workdirMode = if (file.canExecute()) FileMode.BLOB_EXE else FileMode.BLOB
                deltas.add(DiffDelta(
                    DiffStatus.MODIFIED,
                    indexEntryToTreeEntry(entry),
                    TreeEntry(workdirMode, entry.path, workdirOid),
                    entry.path
                ))
            }
        }
    }

    // Find new (untracked) files
    val newFiles = mutableListOf<String>()
    collectDiffFiles(workdir, workdir, gitDir, indexedPaths, newFiles)
    newFiles.sort()

    for (relPath in newFiles) {
        val file = File(workdir, relPath)
        val content = file.readBytes()
        val oid = OID.hashObject(ObjectType.BLOB, content)
        val mode = if (file.canExecute()) FileMode.BLOB_EXE else FileMode.BLOB
        deltas.add(DiffDelta(
            DiffStatus.ADDED,
            null,
            TreeEntry(mode, relPath, oid),
            relPath
        ))
    }

    return deltas
}

private fun indexEntryToTreeEntry(entry: IndexEntry): TreeEntry =
    TreeEntry(entry.mode, entry.path, entry.oid)

private fun collectDiffFiles(dir: File, workdir: File, gitDir: File, indexed: Set<String>, result: MutableList<String>) {
    val items = dir.listFiles() ?: return
    for (item in items) {
        if (item.name == ".git") continue

        if (item.isDirectory) {
            collectDiffFiles(item, workdir, gitDir, indexed, result)
        } else {
            val relative = item.relativeTo(workdir).path
            if (relative !in indexed) {
                result.add(relative)
            }
        }
    }
}
