package ai.muonium.muongit

import java.io.File

/** Options for checkout behavior. */
data class CheckoutOptions(
    /** If true, overwrite existing files in the workdir. */
    val force: Boolean = false,
)

/** Result of a checkout operation. */
data class CheckoutResult(
    /** Files written to the workdir. */
    val updated: MutableList<String> = mutableListOf(),
    /** Files skipped because they already exist (when force is false). */
    val conflicts: MutableList<String> = mutableListOf(),
)

data class SwitchOptions(
    val force: Boolean = false,
)

data class SwitchResult(
    val previousHead: OID?,
    val headOid: OID,
    val headRef: String?,
    val updatedPaths: List<String>,
    val removedPaths: List<String>,
)

enum class ResetMode {
    SOFT,
    MIXED,
    HARD,
}

data class ResetResult(
    val previousHead: OID,
    val headOid: OID,
    val movedRef: String?,
    val updatedPaths: List<String>,
    val removedPaths: List<String>,
)

data class RestoreOptions(
    val source: String? = null,
    val staged: Boolean = false,
    val worktree: Boolean = true,
)

data class RestoreResult(
    val stagedPaths: MutableList<String> = mutableListOf(),
    val removedFromIndex: MutableList<String> = mutableListOf(),
    val restoredPaths: MutableList<String> = mutableListOf(),
    val removedFromWorkdir: MutableList<String> = mutableListOf(),
)

private data class WorkdirUpdate(
    val updatedPaths: MutableList<String> = mutableListOf(),
    val removedPaths: MutableList<String> = mutableListOf(),
)

private data class MaterializedEntry(
    val oid: OID,
    val mode: Int,
    val data: ByteArray,
)

/** Checkout the index to the working directory. */
fun checkoutIndex(gitDir: File, workdir: File, options: CheckoutOptions): CheckoutResult {
    val index = readIndex(gitDir)
    val result = CheckoutResult()

    for (entry in index.entries) {
        checkoutEntry(gitDir, workdir, entry, options, result)
    }

    return result
}

/** Checkout specific paths from the index to the working directory. */
fun checkoutPaths(gitDir: File, workdir: File, paths: List<String>, options: CheckoutOptions): CheckoutResult {
    val index = readIndex(gitDir)
    val result = CheckoutResult()

    for (path in paths) {
        val entry = index.entries.find { it.path == path }
            ?: throw MuonGitException.NotFound("path '$path' not in index")
        checkoutEntry(gitDir, workdir, entry, options, result)
    }

    return result
}

fun switchBranch(
    gitDir: File,
    workdir: File,
    name: String,
    options: SwitchOptions = SwitchOptions(),
): SwitchResult {
    val branch = lookupBranch(gitDir, name, BranchType.LOCAL)
    val targetOid = branch.target
        ?: throw MuonGitException.InvalidSpec("branch '$name' has no target commit")

    val currentIndex = readIndex(gitDir)
    val previousHead = runCatching { currentHeadOid(gitDir) }.getOrNull()
    val currentDesc = describeHead(gitDir)
    val targetEntries = materializeCommitTree(gitDir, targetOid)

    if (!options.force) {
        val conflicts = collectSwitchConflicts(gitDir, workdir, currentIndex, targetEntries)
        if (conflicts.isNotEmpty()) {
            throw MuonGitException.Conflict(
                "checkout would overwrite local changes: ${conflicts.joinToString(", ")}"
            )
        }
    }

    writeIndex(gitDir, indexFromMaterialized(targetEntries))
    val update = applyWorkdirTree(workdir, currentIndex, targetEntries)
    writeSymbolicReference(gitDir, "HEAD", branch.referenceName)

    appendReflog(
        gitDir,
        "HEAD",
        previousHead ?: OID.ZERO,
        targetOid,
        defaultSignature(),
        "checkout: moving from $currentDesc to $name"
    )

    return SwitchResult(
        previousHead = previousHead,
        headOid = targetOid,
        headRef = branch.referenceName,
        updatedPaths = update.updatedPaths,
        removedPaths = update.removedPaths,
    )
}

fun checkoutRevision(
    gitDir: File,
    workdir: File,
    spec: String,
    options: SwitchOptions = SwitchOptions(),
): SwitchResult {
    val targetOid = resolveRevision(gitDir, spec)
    val currentIndex = readIndex(gitDir)
    val previousHead = runCatching { currentHeadOid(gitDir) }.getOrNull()
    val currentDesc = describeHead(gitDir)
    val targetEntries = materializeCommitTree(gitDir, targetOid)

    if (!options.force) {
        val conflicts = collectSwitchConflicts(gitDir, workdir, currentIndex, targetEntries)
        if (conflicts.isNotEmpty()) {
            throw MuonGitException.Conflict(
                "checkout would overwrite local changes: ${conflicts.joinToString(", ")}"
            )
        }
    }

    writeIndex(gitDir, indexFromMaterialized(targetEntries))
    val update = applyWorkdirTree(workdir, currentIndex, targetEntries)
    writeReference(gitDir, "HEAD", targetOid)

    appendReflog(
        gitDir,
        "HEAD",
        previousHead ?: OID.ZERO,
        targetOid,
        defaultSignature(),
        "checkout: moving from $currentDesc to $spec"
    )

    return SwitchResult(
        previousHead = previousHead,
        headOid = targetOid,
        headRef = null,
        updatedPaths = update.updatedPaths,
        removedPaths = update.removedPaths,
    )
}

fun reset(
    gitDir: File,
    workdir: File?,
    spec: String,
    mode: ResetMode,
): ResetResult {
    val targetOid = resolveRevision(gitDir, spec)
    val previousHead = currentHeadOid(gitDir)
    val movedRef = currentHeadTargetRef(gitDir)
    val currentIndex = readIndex(gitDir)

    if (movedRef != null) {
        writeReference(gitDir, movedRef, targetOid)
    } else {
        writeReference(gitDir, "HEAD", targetOid)
    }

    var update = WorkdirUpdate()
    if (mode != ResetMode.SOFT) {
        val targetEntries = materializeCommitTree(gitDir, targetOid)
        writeIndex(gitDir, indexFromMaterialized(targetEntries))

        if (mode == ResetMode.HARD) {
            val resolvedWorkdir = workdir ?: throw MuonGitException.BareRepo()
            update = applyWorkdirTree(resolvedWorkdir, currentIndex, targetEntries)
        }
    }

    val message = "reset: moving to $spec"
    val signature = defaultSignature()
    if (movedRef != null) {
        appendReflog(gitDir, movedRef, previousHead, targetOid, signature, message)
    }
    appendReflog(gitDir, "HEAD", previousHead, targetOid, signature, message)

    return ResetResult(
        previousHead = previousHead,
        headOid = targetOid,
        movedRef = movedRef,
        updatedPaths = update.updatedPaths,
        removedPaths = update.removedPaths,
    )
}

fun restore(
    gitDir: File,
    workdir: File?,
    paths: List<String>,
    options: RestoreOptions = RestoreOptions(),
): RestoreResult {
    val worktreeRequested = options.worktree || !options.staged
    val sourceSpec = if (options.source != null || options.staged) {
        options.source ?: "HEAD"
    } else {
        null
    }
    val sourceEntries = sourceSpec?.let { materializeRevisionTree(gitDir, it) }

    val originalIndex = readIndex(gitDir)
    val index = originalIndex.copy(entries = originalIndex.entries.toMutableList())
    val result = RestoreResult()

    for (path in paths) {
        if (!options.staged) {
            continue
        }

        val entry = sourceEntries?.get(path)
        if (entry != null) {
            index.add(indexEntryFromMaterialized(path, entry))
            result.stagedPaths.add(path)
        } else if (index.remove(path)) {
            result.removedFromIndex.add(path)
        } else {
            throw MuonGitException.NotFound("path '$path' not found in restore source")
        }
    }

    if (options.staged) {
        writeIndex(gitDir, index)
    }

    if (worktreeRequested) {
        val resolvedWorkdir = workdir ?: throw MuonGitException.BareRepo()
        for (path in paths) {
            val knownPath = originalIndex.find(path) != null || File(resolvedWorkdir, path).exists()
            if (sourceEntries != null && options.source != null) {
                restorePathFromMaterialized(resolvedWorkdir, path, sourceEntries[path], knownPath, result)
                continue
            }
            restorePathFromIndex(gitDir, resolvedWorkdir, path, index.find(path), knownPath, result)
        }
    }

    return result
}

fun Repository.checkoutIndex(options: CheckoutOptions): CheckoutResult {
    val resolvedWorkdir = workdir ?: throw MuonGitException.BareRepo()
    return checkoutIndex(gitDir, resolvedWorkdir, options)
}

fun Repository.checkoutPaths(paths: List<String>, options: CheckoutOptions): CheckoutResult {
    val resolvedWorkdir = workdir ?: throw MuonGitException.BareRepo()
    return checkoutPaths(gitDir, resolvedWorkdir, paths, options)
}

fun Repository.switchBranch(name: String, options: SwitchOptions = SwitchOptions()): SwitchResult {
    val resolvedWorkdir = workdir ?: throw MuonGitException.BareRepo()
    return switchBranch(gitDir, resolvedWorkdir, name, options)
}

fun Repository.checkoutRevision(spec: String, options: SwitchOptions = SwitchOptions()): SwitchResult {
    val resolvedWorkdir = workdir ?: throw MuonGitException.BareRepo()
    return checkoutRevision(gitDir, resolvedWorkdir, spec, options)
}

fun Repository.reset(spec: String, mode: ResetMode): ResetResult =
    reset(gitDir, workdir, spec, mode)

fun Repository.restore(paths: List<String>, options: RestoreOptions = RestoreOptions()): RestoreResult =
    restore(gitDir, workdir, paths, options)

private fun checkoutEntry(
    gitDir: File,
    workdir: File,
    entry: IndexEntry,
    options: CheckoutOptions,
    result: CheckoutResult,
) {
    val targetPath = File(workdir, entry.path)

    if (!options.force && targetPath.exists()) {
        result.conflicts.add(entry.path)
        return
    }

    targetPath.parentFile?.mkdirs()
    val blob = readBlob(gitDir, entry.oid)
    targetPath.writeBytes(blob.data)
    setMode(targetPath, entry.mode)

    result.updated.add(entry.path)
}

private fun currentHeadTargetRef(gitDir: File): String? {
    val head = readReference(gitDir, "HEAD")
    return head.removePrefix("ref: ").trim().takeIf { head.startsWith("ref: ") }
}

private fun currentHeadOid(gitDir: File): OID {
    val head = readReference(gitDir, "HEAD")
    return if (head.startsWith("ref: ")) {
        try {
            resolveReference(gitDir, "HEAD")
        } catch (_: MuonGitException.NotFound) {
            throw MuonGitException.UnbornBranch()
        }
    } else {
        OID(head.trim())
    }
}

private fun describeHead(gitDir: File): String {
    val head = readReference(gitDir, "HEAD")
    return if (head.startsWith("ref: ")) {
        val target = head.removePrefix("ref: ").trim()
        if (target.startsWith("refs/heads/")) target.removePrefix("refs/heads/") else target
    } else {
        shortOid(OID(head.trim()))
    }
}

private fun shortOid(oid: OID): String = oid.hex.take(7)

private fun defaultSignature(): Signature =
    Signature(name = "MuonGit", email = "muongit@example.invalid", time = 0L, offset = 0)

private fun materializeRevisionTree(gitDir: File, spec: String): Map<String, MaterializedEntry> =
    materializeCommitTree(gitDir, resolveRevision(gitDir, spec))

private fun materializeCommitTree(gitDir: File, commitOid: OID): Map<String, MaterializedEntry> {
    val commit = revisionReadCommit(gitDir, commitOid)
    val entries = sortedMapOf<String, MaterializedEntry>()
    collectTreeEntries(gitDir, commit.treeId, "", entries)
    return entries
}

private fun collectTreeEntries(
    gitDir: File,
    treeOid: OID,
    prefix: String,
    entries: MutableMap<String, MaterializedEntry>,
) {
    val tree = readObject(gitDir, treeOid).asTree()
    for (entry in tree.entries) {
        val path = if (prefix.isEmpty()) entry.name else "$prefix/${entry.name}"
        if (entry.mode == FileMode.TREE) {
            collectTreeEntries(gitDir, entry.oid, path, entries)
        } else {
            val blob = readBlob(gitDir, entry.oid)
            entries[path] = MaterializedEntry(entry.oid, entry.mode, blob.data)
        }
    }
}

private fun indexFromMaterialized(entries: Map<String, MaterializedEntry>): Index {
    val index = Index()
    for (path in entries.keys.sorted()) {
        val entry = entries[path] ?: continue
        index.add(indexEntryFromMaterialized(path, entry))
    }
    return index
}

private fun indexEntryFromMaterialized(path: String, entry: MaterializedEntry): IndexEntry =
    IndexEntry(
        mode = entry.mode,
        fileSize = entry.data.size,
        oid = entry.oid,
        flags = minOf(path.length, 0x0FFF),
        path = path,
    )

private fun collectSwitchConflicts(
    gitDir: File,
    workdir: File,
    currentIndex: Index,
    targetEntries: Map<String, MaterializedEntry>,
): List<String> {
    val conflicts = sortedSetOf<String>()

    for (path in stagedChangePaths(gitDir, currentIndex)) {
        conflicts += path
    }

    for (path in targetEntries.keys.sorted()) {
        val current = currentIndex.find(path)
        if (current != null) {
            if (!workdirMatchesEntry(workdir, current)) {
                conflicts += path
            }
        } else if (File(workdir, path).exists()) {
            conflicts += path
        }
    }

    for (entry in currentIndex.entries) {
        if (!targetEntries.containsKey(entry.path) && !workdirMatchesEntry(workdir, entry)) {
            conflicts += entry.path
        }
    }

    return conflicts.toList()
}

private fun stagedChangePaths(gitDir: File, currentIndex: Index): List<String> {
    val currentHead = try {
        currentHeadOid(gitDir)
    } catch (_: MuonGitException.UnbornBranch) {
        null
    }
    val headEntries = currentHead?.let { materializeCommitTree(gitDir, it) } ?: emptyMap()
    val currentPaths = currentIndex.entries.map { it.path }.toSet()
    val headPaths = headEntries.keys.toSet()
    val changes = sortedSetOf<String>()

    for (entry in currentIndex.entries) {
        val headEntry = headEntries[entry.path]
        if (headEntry == null || headEntry.oid != entry.oid || headEntry.mode != entry.mode) {
            changes += entry.path
        }
    }

    for (path in headPaths - currentPaths) {
        changes += path
    }

    return changes.toList()
}

private fun workdirMatchesEntry(workdir: File, entry: IndexEntry): Boolean {
    val targetPath = File(workdir, entry.path)
    if (!targetPath.exists() || !targetPath.isFile) {
        return false
    }

    val content = targetPath.readBytes()
    if (content.size != entry.fileSize) {
        return false
    }
    if (OID.hashObject(ObjectType.BLOB, content) != entry.oid) {
        return false
    }

    val isExecutable = targetPath.canExecute()
    val expected = (entry.mode and 0b001001001) != 0
    if (isExecutable != expected) {
        return false
    }

    return true
}

private fun applyWorkdirTree(
    workdir: File,
    currentIndex: Index,
    targetEntries: Map<String, MaterializedEntry>,
): WorkdirUpdate {
    val update = WorkdirUpdate()
    val currentPaths = currentIndex.entries.map { it.path }.toSet()
    val targetPaths = targetEntries.keys.toSet()

    for (path in (currentPaths - targetPaths).sorted()) {
        val filePath = File(workdir, path)
        if (filePath.exists()) {
            removeWorkdirPath(workdir, filePath)
            update.removedPaths += path
        }
    }

    for (path in targetEntries.keys.sorted()) {
        val entry = targetEntries[path] ?: continue
        writeMaterializedToWorkdir(workdir, path, entry)
        update.updatedPaths += path
    }

    return update
}

private fun restorePathFromMaterialized(
    workdir: File,
    path: String,
    entry: MaterializedEntry?,
    knownPath: Boolean,
    result: RestoreResult,
) {
    if (entry != null) {
        writeMaterializedToWorkdir(workdir, path, entry)
        result.restoredPaths += path
        return
    }

    val target = File(workdir, path)
    if (target.exists()) {
        removeWorkdirPath(workdir, target)
        result.removedFromWorkdir += path
    } else if (!knownPath) {
        throw MuonGitException.NotFound("path '$path' not found")
    }
}

private fun restorePathFromIndex(
    gitDir: File,
    workdir: File,
    path: String,
    entry: IndexEntry?,
    knownPath: Boolean,
    result: RestoreResult,
) {
    if (entry != null) {
        writeIndexEntryToWorkdir(gitDir, workdir, entry)
        result.restoredPaths += path
        return
    }

    val target = File(workdir, path)
    if (target.exists()) {
        removeWorkdirPath(workdir, target)
        result.removedFromWorkdir += path
    } else if (!knownPath) {
        throw MuonGitException.NotFound("path '$path' not found")
    }
}

private fun writeMaterializedToWorkdir(workdir: File, path: String, entry: MaterializedEntry) {
    val target = File(workdir, path)
    target.parentFile?.mkdirs()
    target.writeBytes(entry.data)
    setMode(target, entry.mode)
}

private fun writeIndexEntryToWorkdir(gitDir: File, workdir: File, entry: IndexEntry) {
    val target = File(workdir, entry.path)
    target.parentFile?.mkdirs()
    val blob = readBlob(gitDir, entry.oid)
    target.writeBytes(blob.data)
    setMode(target, entry.mode)
}

private fun setMode(path: File, mode: Int) {
    val executable = (mode and 0b001001001) != 0
    path.setExecutable(executable, false)
    path.setReadable(true, false)
    path.setWritable(true, true)
}

private fun removeWorkdirPath(root: File, path: File) {
    if (path.exists()) {
        if (path.isDirectory) {
            path.deleteRecursively()
        } else {
            path.delete()
        }
    }

    var current = path.parentFile
    while (current != null && current != root) {
        val contents = current.list()
        if (contents != null && contents.isNotEmpty()) {
            break
        }
        current.delete()
        current = current.parentFile
    }
}
