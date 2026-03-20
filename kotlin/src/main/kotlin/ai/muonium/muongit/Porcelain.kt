package ai.muonium.muongit

import java.io.File

data class AddOptions(
    val includeIgnored: Boolean = false
)

data class AddResult(
    val stagedPaths: List<String> = emptyList(),
    val removedPaths: List<String> = emptyList()
)

data class RemoveResult(
    val removedFromIndex: List<String> = emptyList(),
    val removedFromWorkdir: List<String> = emptyList()
)

data class UnstageResult(
    val restoredPaths: List<String> = emptyList(),
    val removedPaths: List<String> = emptyList()
)

data class CommitOptions(
    val author: Signature? = null,
    val committer: Signature? = null
)

data class CommitResult(
    val oid: OID,
    val treeId: OID,
    val parentIds: List<OID>,
    val reference: String,
    val summary: String
)

fun addPaths(
    gitDir: File,
    workdir: File,
    patterns: List<String>,
    options: AddOptions = AddOptions()
): AddResult {
    val index = readIndex(gitDir)
    val candidates = collectWorkdirPaths(gitDir, workdir, options.includeIgnored).toMutableSet()
    index.entries.forEach { candidates.add(it.path) }
    val matched = matchPatterns(candidates, patterns)
    val staged = mutableListOf<String>()
    val removed = mutableListOf<String>()

    for (path in matched) {
        val fullPath = File(workdir, path)
        if (fullPath.isFile) {
            stagePath(gitDir, workdir, index, path)
            staged.add(path)
        } else if (index.remove(path)) {
            removed.add(path)
        }
    }

    writeIndex(gitDir, index)
    return AddResult(stagedPaths = staged, removedPaths = removed)
}

fun removePaths(
    gitDir: File,
    workdir: File,
    patterns: List<String>
): RemoveResult {
    val index = readIndex(gitDir)
    val candidates = collectWorkdirPaths(gitDir, workdir, includeIgnored = true).toMutableSet()
    index.entries.forEach { candidates.add(it.path) }
    val matched = matchPatterns(candidates, patterns)
    val removedFromIndex = mutableListOf<String>()
    val removedFromWorkdir = mutableListOf<String>()

    for (path in matched) {
        if (index.remove(path)) {
            removedFromIndex.add(path)
        }

        val fullPath = File(workdir, path)
        if (fullPath.exists()) {
            removeWorkdirPath(workdir, fullPath)
            removedFromWorkdir.add(path)
        }
    }

    writeIndex(gitDir, index)
    return RemoveResult(removedFromIndex = removedFromIndex, removedFromWorkdir = removedFromWorkdir)
}

fun unstagePaths(gitDir: File, patterns: List<String>): UnstageResult {
    val index = readIndex(gitDir)
    val headEntries = readHeadIndexEntries(gitDir)
    val candidates = (index.entries.map { it.path } + headEntries.keys).toSortedSet()
    val matched = matchPatterns(candidates, patterns)
    val restored = mutableListOf<String>()
    val removed = mutableListOf<String>()

    for (path in matched) {
        val headEntry = headEntries[path]
        if (headEntry != null) {
            index.add(headEntry)
            restored.add(path)
        } else if (index.remove(path)) {
            removed.add(path)
        }
    }

    writeIndex(gitDir, index)
    return UnstageResult(restoredPaths = restored, removedPaths = removed)
}

fun createCommit(
    gitDir: File,
    message: String,
    options: CommitOptions = CommitOptions()
): CommitResult {
    val headRef = currentHeadRef(gitDir)
        ?: throw MuonGitException.InvalidSpec("cannot commit on detached HEAD")
    val parentOid = try {
        resolveReference(gitDir, "HEAD")
    } catch (_: MuonGitException.NotFound) {
        null
    }
    val index = readIndex(gitDir)
    val treeId = writeTreeFromIndex(gitDir, index)
    val author = options.author ?: defaultSignature()
    val committer = options.committer ?: author
    val normalizedMessage = normalizeCommitMessage(message)
    val summary = commitSummary(normalizedMessage)
    val parentIds = parentOid?.let(::listOf) ?: emptyList()
    val data = serializeCommit(treeId, parentIds, author, committer, normalizedMessage)
    val commitOid = writeLooseObject(gitDir, ObjectType.COMMIT, data)
    writeReference(gitDir, headRef, commitOid)

    val oldOid = parentOid ?: OID.ZERO
    val reflogMessage = if (oldOid.isZero) {
        "commit (initial): $summary"
    } else {
        "commit: $summary"
    }
    appendReflog(gitDir, headRef, oldOid, commitOid, committer, reflogMessage)
    appendReflog(gitDir, "HEAD", oldOid, commitOid, committer, reflogMessage)

    return CommitResult(
        oid = commitOid,
        treeId = treeId,
        parentIds = parentIds,
        reference = headRef,
        summary = summary
    )
}

fun Repository.add(paths: List<String>, options: AddOptions = AddOptions()): AddResult {
    val resolvedWorkdir = workdir ?: throw MuonGitException.BareRepo()
    return addPaths(gitDir, resolvedWorkdir, paths, options)
}

fun Repository.remove(paths: List<String>): RemoveResult {
    val resolvedWorkdir = workdir ?: throw MuonGitException.BareRepo()
    return removePaths(gitDir, resolvedWorkdir, paths)
}

fun Repository.unstage(paths: List<String>): UnstageResult =
    unstagePaths(gitDir, paths)

fun Repository.commit(message: String, options: CommitOptions = CommitOptions()): CommitResult =
    createCommit(gitDir, message, options)

private fun matchPatterns(candidates: Set<String>, patterns: List<String>): List<String> {
    val ordered = candidates.sorted()
    if (ordered.isEmpty()) {
        throw MuonGitException.NotFound("no paths available")
    }
    if (patterns.isEmpty()) {
        return ordered
    }

    val pathspec = Pathspec(patterns)
    val matches = pathspec.matchPaths(ordered).matches
    val failures = patterns.filter { pattern ->
        Pathspec(listOf(pattern)).matchPaths(ordered).matches.isEmpty()
    }
    if (failures.isNotEmpty()) {
        throw MuonGitException.NotFound("pathspec did not match: ${failures.joinToString(", ")}")
    }
    return matches
}

private fun collectWorkdirPaths(
    gitDir: File,
    workdir: File,
    includeIgnored: Boolean
): Set<String> {
    val paths = sortedSetOf<String>()
    collectWorkdirPathsRecursive(workdir, workdir, gitDir, includeIgnored, paths)
    return paths
}

private fun collectWorkdirPathsRecursive(
    dir: File,
    workdir: File,
    gitDir: File,
    includeIgnored: Boolean,
    paths: MutableSet<String>
) {
    val ignore = ignoreForDirectory(gitDir, workdir, relativePath(dir, workdir))
    val children = dir.listFiles()?.sortedBy { it.name } ?: return

    for (child in children) {
        if (child == gitDir || child.name == ".git") {
            continue
        }

        val relPath = relativePath(child, workdir)
        if (child.isDirectory) {
            if (!includeIgnored && ignore.isIgnored(relPath, isDir = true)) {
                continue
            }
            collectWorkdirPathsRecursive(child, workdir, gitDir, includeIgnored, paths)
        } else if (child.isFile) {
            if (!includeIgnored && ignore.isIgnored(relPath, isDir = false)) {
                continue
            }
            paths.add(relPath)
        }
    }
}

private fun ignoreForDirectory(gitDir: File, workdir: File, relDir: String): Ignore {
    val ignore = Ignore.load(gitDir, workdir)
    var current = ""
    for (part in relDir.split('/').filter { it.isNotEmpty() }) {
        current = if (current.isEmpty()) part else "$current/$part"
        ignore.loadForPath(workdir, current)
    }
    return ignore
}

private fun relativePath(path: File, workdir: File): String {
    if (path == workdir) return ""
    val rel = path.relativeTo(workdir).invariantSeparatorsPath
    if (rel.startsWith("..")) {
        throw MuonGitException.InvalidSpec("path is outside repository workdir")
    }
    return rel
}

private fun stagePath(
    gitDir: File,
    workdir: File,
    index: Index,
    path: String
) {
    val file = File(workdir, path)
    val raw = file.readBytes()
    val filtered = FilterList.load(gitDir, workdir, path, FilterMode.TO_ODB).apply(raw)
    val oid = writeLooseObject(gitDir, ObjectType.BLOB, filtered)
    val mode = if (file.canExecute()) FileMode.BLOB_EXE else FileMode.BLOB

    index.add(
        IndexEntry(
            mode = mode,
            fileSize = file.length().toInt(),
            oid = oid,
            flags = minOf(path.length, 0x0FFF),
            path = path
        )
    )
}

private fun readHeadIndexEntries(gitDir: File): Map<String, IndexEntry> {
    val headOid = try {
        resolveReference(gitDir, "HEAD")
    } catch (_: MuonGitException.NotFound) {
        return emptyMap()
    }
    val commit = readObject(gitDir, headOid).asCommit()
    val entries = mutableMapOf<String, IndexEntry>()
    collectHeadTreeEntries(gitDir, commit.treeId, "", entries)
    return entries
}

private fun collectHeadTreeEntries(
    gitDir: File,
    treeOid: OID,
    prefix: String,
    entries: MutableMap<String, IndexEntry>
) {
    val tree = readObject(gitDir, treeOid).asTree()
    for (entry in tree.entries) {
        val path = if (prefix.isEmpty()) entry.name else "$prefix/${entry.name}"
        if (entry.mode == FileMode.TREE) {
            collectHeadTreeEntries(gitDir, entry.oid, path, entries)
        } else {
            val blob = readBlob(gitDir, entry.oid)
            entries[path] = IndexEntry(
                mode = entry.mode,
                fileSize = blob.data.size,
                oid = entry.oid,
                flags = minOf(path.length, 0x0FFF),
                path = path
            )
        }
    }
}

private class TreeNode {
    val files = mutableListOf<TreeEntry>()
    val children = sortedMapOf<String, TreeNode>()
}

internal fun writeTreeFromIndex(gitDir: File, index: Index): OID {
    val root = TreeNode()
    for (entry in index.entries) {
        insertTreeEntry(root, entry)
    }
    return writeTreeNode(gitDir, root)
}

private fun insertTreeEntry(node: TreeNode, entry: IndexEntry) {
    val parts = entry.path.split('/').filter { it.isNotEmpty() }
    if (parts.isEmpty()) {
        throw MuonGitException.InvalidSpec("empty index path")
    }
    insertTreeEntryParts(node, entry, parts, 0)
}

private fun insertTreeEntryParts(node: TreeNode, entry: IndexEntry, parts: List<String>, depth: Int) {
    val part = parts[depth]
    if (depth == parts.lastIndex) {
        node.files.add(TreeEntry(mode = entry.mode, name = part, oid = entry.oid))
        return
    }
    val child = node.children.getOrPut(part) { TreeNode() }
    insertTreeEntryParts(child, entry, parts, depth + 1)
}

private fun writeTreeNode(gitDir: File, node: TreeNode): OID {
    val entries = node.files.toMutableList()
    for ((name, child) in node.children) {
        val childOid = writeTreeNode(gitDir, child)
        entries.add(TreeEntry(mode = FileMode.TREE, name = name, oid = childOid))
    }
    return writeLooseObject(gitDir, ObjectType.TREE, serializeTree(entries))
}

private fun currentHeadRef(gitDir: File): String? {
    val head = readReference(gitDir, "HEAD")
    return if (head.startsWith("ref: ")) head.removePrefix("ref: ").trim() else null
}

private fun normalizeCommitMessage(message: String): String =
    if (message.endsWith('\n')) message else "$message\n"

private fun commitSummary(message: String): String =
    message.lineSequence().firstOrNull().orEmpty()

private fun defaultSignature(): Signature =
    Signature(name = "MuonGit", email = "muongit@example.invalid")

private fun removeWorkdirPath(workdir: File, target: File) {
    target.deleteRecursively()
    pruneEmptyParents(workdir, target.parentFile)
}

private fun pruneEmptyParents(workdir: File, current: File?) {
    var path = current
    while (path != null && path != workdir) {
        val children = path.listFiles() ?: emptyArray()
        if (children.isNotEmpty()) {
            break
        }
        val parent = path.parentFile
        path.delete()
        path = parent
    }
}
