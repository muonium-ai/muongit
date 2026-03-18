package ai.muonium.muongit

import java.io.File

/** Status of a file in the working directory */
enum class FileStatus {
    DELETED,
    NEW,
    MODIFIED,
}

/** A single status entry */
data class StatusEntry(
    val path: String,
    val status: FileStatus,
)

/** Compute the working directory status by comparing the index against the workdir. */
fun workdirStatus(gitDir: File, workdir: File): List<StatusEntry> {
    val index = readIndex(gitDir)
    val entries = mutableListOf<StatusEntry>()

    val indexedPaths = index.entries.map { it.path }.toSet()

    // Check each index entry against the working directory
    for (entry in index.entries) {
        val file = File(workdir, entry.path)
        if (!file.exists()) {
            entries.add(StatusEntry(entry.path, FileStatus.DELETED))
        } else if (isModified(file, entry)) {
            entries.add(StatusEntry(entry.path, FileStatus.MODIFIED))
        }
    }

    // Find new (untracked) files
    val newFiles = mutableListOf<String>()
    collectFiles(workdir, workdir, gitDir, indexedPaths, newFiles)
    newFiles.sort()
    for (path in newFiles) {
        entries.add(StatusEntry(path, FileStatus.NEW))
    }

    return entries
}

private fun isModified(file: File, entry: IndexEntry): Boolean {
    val fileSize = file.length().toInt()
    if (fileSize != entry.fileSize) return true

    val content = file.readBytes()
    val oid = OID.hashObject(ObjectType.BLOB, content)
    return oid != entry.oid
}

private fun collectFiles(dir: File, workdir: File, gitDir: File, indexed: Set<String>, result: MutableList<String>) {
    val items = dir.listFiles() ?: return
    for (item in items) {
        if (item.name == ".git") continue

        if (item.isDirectory) {
            collectFiles(item, workdir, gitDir, indexed, result)
        } else {
            val relative = item.relativeTo(workdir).path
            if (relative !in indexed) {
                result.add(relative)
            }
        }
    }
}
