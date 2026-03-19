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

private fun checkoutEntry(
    gitDir: File,
    workdir: File,
    entry: IndexEntry,
    options: CheckoutOptions,
    result: CheckoutResult,
) {
    val targetPath = File(workdir, entry.path)

    // Check for existing file when not forcing
    if (!options.force && targetPath.exists()) {
        result.conflicts.add(entry.path)
        return
    }

    // Create parent directories
    targetPath.parentFile?.mkdirs()

    // Read blob content
    val blob = readBlob(gitDir, entry.oid)

    // Write file
    targetPath.writeBytes(blob.data)

    // Set file permissions based on mode
    val isExecutable = (entry.mode and 0b001001001) != 0 // 0o111
    targetPath.setExecutable(isExecutable, false)
    targetPath.setReadable(true, false)
    targetPath.setWritable(true, true)

    result.updated.add(entry.path)
}
