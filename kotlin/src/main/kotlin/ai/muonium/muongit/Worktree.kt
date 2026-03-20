// Git worktree support — multiple working trees for a single repository.
// Parity: libgit2 src/libgit2/worktree.c

package ai.muonium.muongit

import java.io.File

/** A linked worktree entry. */
data class Worktree(
    /** Name of the worktree (basename under .git/worktrees/). */
    val name: String,
    /** Filesystem path to the worktree working directory. */
    val path: String,
    /** Path to the worktree's gitdir inside the parent's .git/worktrees/<name>/. */
    val gitdirPath: String,
    /** Whether this worktree is locked. */
    val locked: Boolean
)

/** Options for creating a new worktree. */
data class WorktreeAddOptions(
    /** Lock the newly created worktree immediately. */
    val lock: Boolean = false,
    /** Branch reference (e.g. "refs/heads/feature"). If null, creates a new
     *  branch named after the worktree pointing at HEAD. */
    val reference: String? = null
)

/** Options controlling worktree prune behavior. */
data class WorktreePruneOptions(
    /** Prune even if the worktree is valid (on-disk data exists). */
    val valid: Boolean = false,
    /** Prune even if the worktree is locked. */
    val locked: Boolean = false,
    /** Also remove the working tree directory. */
    val workingTree: Boolean = false
)

/** List names of linked worktrees for a repository. */
fun worktreeList(gitDir: File): List<String> {
    val worktreesDir = File(gitDir, "worktrees")
    if (!worktreesDir.isDirectory) return emptyList()

    return worktreesDir.listFiles()
        ?.filter { it.isDirectory && isWorktreeDir(it) }
        ?.map { it.name }
        ?.sorted()
        ?: emptyList()
}

/** Look up a linked worktree by name. */
fun worktreeLookup(gitDir: File, name: String): Worktree {
    val wtDir = File(gitDir, "worktrees/$name")
    if (!wtDir.isDirectory) {
        throw MuonGitException.NotFound("worktree '$name' not found")
    }
    if (!isWorktreeDir(wtDir)) {
        throw MuonGitException.InvalidSpec("worktree '$name' has invalid structure")
    }
    return openWorktree(gitDir, name)
}

/** Validate that a worktree's on-disk structure is intact. */
fun worktreeValidate(worktree: Worktree) {
    val gitdirFile = File(worktree.gitdirPath)
    if (!gitdirFile.isDirectory) {
        throw MuonGitException.NotFound("worktree gitdir missing: ${worktree.gitdirPath}")
    }
    if (!isWorktreeDir(gitdirFile)) {
        throw MuonGitException.InvalidSpec("worktree '${worktree.name}' has invalid gitdir structure")
    }
    if (!File(worktree.path).isDirectory) {
        throw MuonGitException.NotFound("worktree working directory missing: ${worktree.path}")
    }
}

/** Add a new linked worktree. */
fun worktreeAdd(
    gitDir: File,
    name: String,
    worktreePath: File,
    options: WorktreeAddOptions = WorktreeAddOptions()
): Worktree {
    val wtMeta = File(gitDir, "worktrees/$name")
    if (wtMeta.exists()) {
        throw MuonGitException.Conflict("worktree '$name' already exists")
    }

    // Determine the branch ref
    val branchRef: String
    if (options.reference != null) {
        branchRef = options.reference
    } else {
        val headOid = resolveReference(gitDir, "HEAD")
        val newBranch = "refs/heads/$name"
        writeReference(gitDir, newBranch, headOid)
        branchRef = newBranch
    }

    // Create metadata dir and worktree dir
    wtMeta.mkdirs()
    worktreePath.mkdirs()

    val absWorktree = worktreePath.canonicalFile

    // Write gitdir file (points to worktree's .git file)
    val gitfileInWt = File(absWorktree, ".git")
    File(wtMeta, "gitdir").writeText("${gitfileInWt.absolutePath}\n")

    // Write commondir file
    File(wtMeta, "commondir").writeText("../..\n")

    // Write HEAD as symbolic ref
    File(wtMeta, "HEAD").writeText("ref: $branchRef\n")

    // Create .git file in worktree (gitlink pointing back to metadata)
    val absWtMeta = wtMeta.canonicalFile
    gitfileInWt.writeText("gitdir: ${absWtMeta.absolutePath}\n")

    // Lock if requested
    if (options.lock) {
        File(wtMeta, "locked").writeText("")
    }

    return Worktree(
        name = name,
        path = absWorktree.absolutePath,
        gitdirPath = absWtMeta.absolutePath,
        locked = options.lock
    )
}

/** Lock a worktree with an optional reason. */
fun worktreeLock(gitDir: File, name: String, reason: String? = null) {
    val wtMeta = File(gitDir, "worktrees/$name")
    if (!wtMeta.exists()) {
        throw MuonGitException.NotFound("worktree '$name' not found")
    }
    val lockFile = File(wtMeta, "locked")
    if (lockFile.exists()) {
        throw MuonGitException.Locked("worktree '$name' is already locked")
    }
    lockFile.writeText(reason ?: "")
}

/** Unlock a worktree. Returns true if was locked, false if was not. */
fun worktreeUnlock(gitDir: File, name: String): Boolean {
    val lockFile = File(gitDir, "worktrees/$name/locked")
    return if (lockFile.exists()) {
        lockFile.delete()
        true
    } else {
        false
    }
}

/** Check whether a worktree is locked. Returns the lock reason if locked, null otherwise. */
fun worktreeIsLocked(gitDir: File, name: String): String? {
    val lockFile = File(gitDir, "worktrees/$name/locked")
    if (!lockFile.exists()) return null
    return lockFile.readText().trim()
}

/** Check if a worktree can be pruned with the given options. */
fun worktreeIsPrunable(
    gitDir: File,
    name: String,
    options: WorktreePruneOptions = WorktreePruneOptions()
): Boolean {
    val wt = worktreeLookup(gitDir, name)
    if (wt.locked && !options.locked) return false
    if (File(wt.path).isDirectory && !options.valid) return false
    return true
}

/** Prune (remove) a worktree's metadata. Optionally removes the working directory. */
fun worktreePrune(
    gitDir: File,
    name: String,
    options: WorktreePruneOptions = WorktreePruneOptions()
) {
    val wt = worktreeLookup(gitDir, name)

    if (wt.locked && !options.locked) {
        throw MuonGitException.Locked("worktree '$name' is locked")
    }
    if (File(wt.path).isDirectory && !options.valid) {
        throw MuonGitException.Conflict("worktree '$name' is still valid; use valid flag to override")
    }

    // Remove working tree directory if requested
    if (options.workingTree) {
        val wtDir = File(wt.path)
        if (wtDir.exists()) wtDir.deleteRecursively()
    }

    // Remove metadata directory
    val wtMeta = File(gitDir, "worktrees/$name")
    if (wtMeta.exists()) wtMeta.deleteRecursively()

    // Clean up worktrees dir if empty
    val worktreesDir = File(gitDir, "worktrees")
    if (worktreesDir.isDirectory && (worktreesDir.listFiles()?.isEmpty() == true)) {
        worktreesDir.delete()
    }
}

// --- Internal helpers ---

private fun isWorktreeDir(path: File): Boolean {
    return File(path, "gitdir").exists()
        && File(path, "commondir").exists()
        && File(path, "HEAD").exists()
}

private fun openWorktree(gitDir: File, name: String): Worktree {
    val wtDir = File(gitDir, "worktrees/$name")
    val gitdirContent = File(wtDir, "gitdir").readText().trim()

    // The worktree path is the parent of the .git file referenced in gitdir
    val worktreePath = File(gitdirContent).parent ?: ""

    val locked = File(wtDir, "locked").exists()

    return Worktree(
        name = name,
        path = worktreePath,
        gitdirPath = wtDir.absolutePath,
        locked = locked
    )
}
