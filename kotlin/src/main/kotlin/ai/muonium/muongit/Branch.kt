package ai.muonium.muongit

import java.io.File

enum class BranchType {
    LOCAL,
    REMOTE,
}

data class BranchUpstream(
    val remoteName: String,
    val mergeRef: String,
)

data class Branch(
    val name: String,
    val referenceName: String,
    val target: OID?,
    val kind: BranchType,
    val isHead: Boolean,
    val upstream: BranchUpstream?,
)

fun createBranch(gitDir: File, name: String, target: OID? = null, force: Boolean = false): Branch {
    val refName = localBranchRef(name)
    val refdb = RefDb(gitDir)

    if (referenceExists(gitDir, refName)) {
        if (!force) {
            throw MuonGitException.Conflict("branch '$name' already exists")
        }
        refdb.delete(refName)
    }

    val targetOid = target ?: headTargetOID(gitDir)
    refdb.write(refName, targetOid)
    return lookupBranch(gitDir, name, BranchType.LOCAL)
}

fun lookupBranch(gitDir: File, name: String, kind: BranchType): Branch =
    buildBranch(gitDir, branchRefName(name, kind), kind)

fun listBranches(gitDir: File, kind: BranchType? = null): List<Branch> {
    val refs = listReferences(gitDir)
    val branches = mutableListOf<Branch>()
    for ((refName, _) in refs) {
        val branchType = branchType(refName) ?: continue
        if (kind == null || kind == branchType) {
            branches += buildBranch(gitDir, refName, branchType)
        }
    }
    return branches.sortedBy { it.referenceName }
}

fun renameBranch(gitDir: File, oldName: String, newName: String, force: Boolean = false): Branch {
    val oldRef = localBranchRef(oldName)
    val newRef = localBranchRef(newName)
    if (oldRef == newRef) {
        return lookupBranch(gitDir, newName, BranchType.LOCAL)
    }

    val refdb = RefDb(gitDir)
    val oldBranch = refdb.read(oldRef)
    if (referenceExists(gitDir, newRef)) {
        if (!force) {
            throw MuonGitException.Conflict("branch '$newName' already exists")
        }
        if (currentHeadRef(gitDir) == newRef) {
            throw MuonGitException.Conflict("cannot replace checked out branch '$newName'")
        }
        deleteBranch(gitDir, newName, BranchType.LOCAL)
    }

    when {
        oldBranch.symbolicTarget != null -> refdb.writeSymbolic(newRef, oldBranch.symbolicTarget)
        oldBranch.target != null -> refdb.write(newRef, oldBranch.target)
        else -> throw MuonGitException.InvalidObject("branch '$oldName' has no target")
    }
    refdb.delete(oldRef)
    moveBranchUpstream(gitDir, oldName, newName)

    if (currentHeadRef(gitDir) == oldRef) {
        refdb.writeSymbolic("HEAD", newRef)
    }

    return lookupBranch(gitDir, newName, BranchType.LOCAL)
}

fun deleteBranch(gitDir: File, name: String, kind: BranchType): Boolean {
    val refName = branchRefName(name, kind)
    if (kind == BranchType.LOCAL && currentHeadRef(gitDir) == refName) {
        throw MuonGitException.Conflict("cannot delete checked out branch '$name'")
    }

    val deleted = RefDb(gitDir).delete(refName)
    if (deleted && kind == BranchType.LOCAL) {
        clearBranchUpstream(gitDir, name)
    }
    return deleted
}

fun branchUpstream(gitDir: File, name: String): BranchUpstream? {
    val config = loadRepoConfig(gitDir)
    val section = branchSection(name)
    val remote = config.get(section, "remote")
    val merge = config.get(section, "merge")
    return when {
        remote != null && merge != null -> BranchUpstream(remote, merge)
        remote == null && merge == null -> null
        else -> throw MuonGitException.InvalidSpec("branch '$name' has incomplete upstream config")
    }
}

fun setBranchUpstream(gitDir: File, name: String, upstream: BranchUpstream?) {
    val refName = localBranchRef(name)
    if (!referenceExists(gitDir, refName)) {
        throw MuonGitException.NotFound("branch '$name' not found")
    }

    val config = loadRepoConfig(gitDir)
    val section = branchSection(name)
    if (upstream != null) {
        config.set(section, "remote", upstream.remoteName)
        config.set(section, "merge", upstream.mergeRef)
    } else {
        config.unset(section, "remote")
        config.unset(section, "merge")
    }
    config.save()
}

fun Repository.createBranch(name: String, target: OID? = null, force: Boolean = false): Branch =
    createBranch(gitDir, name, target, force)

fun Repository.lookupBranch(name: String, kind: BranchType): Branch =
    lookupBranch(gitDir, name, kind)

fun Repository.listBranches(kind: BranchType? = null): List<Branch> =
    listBranches(gitDir, kind)

private fun buildBranch(gitDir: File, refName: String, kind: BranchType): Branch {
    val reference = RefDb(gitDir).read(refName)
    val target = if (reference.isSymbolic) {
        try {
            resolveReference(gitDir, refName)
        } catch (_: Exception) {
            null
        }
    } else {
        reference.target
    }
    val shortName = shortBranchName(refName, kind)
        ?: throw MuonGitException.InvalidSpec("not a branch reference: $refName")

    return Branch(
        name = shortName,
        referenceName = refName,
        target = target,
        kind = kind,
        isHead = currentHeadRef(gitDir) == refName,
        upstream = if (kind == BranchType.LOCAL) branchUpstream(gitDir, shortName) else null,
    )
}

private fun branchRefName(name: String, kind: BranchType): String = when (kind) {
    BranchType.LOCAL -> localBranchRef(name)
    BranchType.REMOTE -> "refs/remotes/$name"
}

private fun localBranchRef(name: String): String = "refs/heads/$name"

private fun shortBranchName(refName: String, kind: BranchType): String? = when (kind) {
    BranchType.LOCAL -> refName.removePrefix("refs/heads/").takeIf { refName.startsWith("refs/heads/") }
    BranchType.REMOTE -> refName.removePrefix("refs/remotes/").takeIf { refName.startsWith("refs/remotes/") }
}

private fun branchType(refName: String): BranchType? = when {
    refName.startsWith("refs/heads/") -> BranchType.LOCAL
    refName.startsWith("refs/remotes/") -> BranchType.REMOTE
    else -> null
}

private fun currentHeadRef(gitDir: File): String? {
    val head = readReference(gitDir, "HEAD")
    if (!head.startsWith("ref: ")) {
        return null
    }
    val target = head.removePrefix("ref: ").trim()
    return target.takeIf { it.startsWith("refs/heads/") }
}

private fun headTargetOID(gitDir: File): OID {
    val head = readReference(gitDir, "HEAD")
    return if (head.startsWith("ref: ")) {
        try {
            resolveReference(gitDir, "HEAD")
        } catch (err: MuonGitException.NotFound) {
            throw MuonGitException.UnbornBranch()
        }
    } else {
        OID(head.trim())
    }
}

private fun branchSection(name: String): String = "branch.$name"

private fun loadRepoConfig(gitDir: File): Config {
    val configPath = File(gitDir, "config")
    return if (configPath.exists()) {
        Config.load(configPath.path)
    } else {
        Config(configPath.path)
    }
}

private fun clearBranchUpstream(gitDir: File, name: String) {
    val configPath = File(gitDir, "config")
    if (!configPath.exists()) {
        return
    }
    val config = Config.load(configPath.path)
    val section = branchSection(name)
    config.unset(section, "remote")
    config.unset(section, "merge")
    config.save()
}

private fun moveBranchUpstream(gitDir: File, oldName: String, newName: String) {
    val upstream = branchUpstream(gitDir, oldName)
    clearBranchUpstream(gitDir, oldName)
    if (upstream != null) {
        setBranchUpstream(gitDir, newName, upstream)
    }
}

private fun referenceExists(gitDir: File, name: String): Boolean =
    try {
        readReference(gitDir, name)
        true
    } catch (_: MuonGitException.NotFound) {
        false
    }
