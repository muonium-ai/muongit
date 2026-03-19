package ai.muonium.muongit

import java.io.File

/** A git remote (e.g. "origin"). */
data class Remote(
    val name: String,
    val url: String,
    val pushUrl: String? = null,
    val fetchRefspecs: List<String> = emptyList(),
)

/** List all remote names from the repository config. */
fun listRemotes(gitDir: File): List<String> {
    val configPath = File(gitDir, "config").absolutePath
    val config = Config.load(configPath)
    val names = mutableListOf<String>()

    for (entry in config.allEntries) {
        val sLower = entry.first.lowercase()
        if (sLower.startsWith("remote.") && entry.second.lowercase() == "url") {
            val name = entry.first.removePrefix("remote.").removePrefix("Remote.")
            val actualName = entry.first.substring("remote.".length)
            if (actualName.isNotEmpty() && actualName !in names) {
                names.add(actualName)
            }
        }
    }

    return names
}

/** Get a remote by name from the repository config. */
fun getRemote(gitDir: File, name: String): Remote {
    val configPath = File(gitDir, "config").absolutePath
    val config = Config.load(configPath)
    val section = "remote.$name"

    val url = config.get(section, "url")
        ?: throw MuonGitException.NotFound("remote '$name' not found")

    val pushUrl = config.get(section, "pushurl")

    val fetchRefspecs = config.allEntries
        .filter { it.first.lowercase() == section.lowercase() && it.second.lowercase() == "fetch" }
        .map { it.third }

    return Remote(name = name, url = url, pushUrl = pushUrl, fetchRefspecs = fetchRefspecs)
}

/** Add a new remote to the repository config. */
fun addRemote(gitDir: File, name: String, url: String): Remote {
    val configPath = File(gitDir, "config").absolutePath
    val config = Config.load(configPath)
    val section = "remote.$name"

    if (config.get(section, "url") != null) {
        throw MuonGitException.InvalidSpec("remote '$name' already exists")
    }

    val fetchRefspec = "+refs/heads/*:refs/remotes/$name/*"
    config.set(section, "url", url)
    config.set(section, "fetch", fetchRefspec)
    config.save()

    return Remote(name = name, url = url, fetchRefspecs = listOf(fetchRefspec))
}

/** Remove a remote from the repository config. */
fun removeRemote(gitDir: File, name: String) {
    val configPath = File(gitDir, "config").absolutePath
    val config = Config.load(configPath)
    val section = "remote.$name"

    if (config.get(section, "url") == null) {
        throw MuonGitException.NotFound("remote '$name' not found")
    }

    config.unset(section, "url")
    config.unset(section, "pushurl")
    config.unset(section, "fetch")
    config.save()
}

/** Rename a remote in the repository config. */
fun renameRemote(gitDir: File, oldName: String, newName: String) {
    val remote = getRemote(gitDir, oldName)

    val configPath = File(gitDir, "config").absolutePath
    val config = Config.load(configPath)
    val oldSection = "remote.$oldName"
    val newSection = "remote.$newName"

    if (config.get(newSection, "url") != null) {
        throw MuonGitException.InvalidSpec("remote '$newName' already exists")
    }

    config.unset(oldSection, "url")
    config.unset(oldSection, "pushurl")
    config.unset(oldSection, "fetch")

    config.set(newSection, "url", remote.url)
    remote.pushUrl?.let { config.set(newSection, "pushurl", it) }
    val newFetch = "+refs/heads/*:refs/remotes/$newName/*"
    config.set(newSection, "fetch", newFetch)
    config.save()
}

/**
 * Parse a refspec string into its components.
 * Format: [+]<src>:<dst>
 * Returns Triple(force, src, dst) or null if malformed.
 */
fun parseRefspec(refspec: String): Triple<Boolean, String, String>? {
    var rest = refspec
    var force = false
    if (rest.startsWith("+")) {
        force = true
        rest = rest.substring(1)
    }
    val colonIdx = rest.indexOf(':')
    if (colonIdx < 0) return null
    val src = rest.substring(0, colonIdx)
    val dst = rest.substring(colonIdx + 1)
    return Triple(force, src, dst)
}
