// Git submodule support
// Parity: libgit2 src/libgit2/submodule.c

package ai.muonium.muongit

import java.io.File

/**
 * A parsed submodule entry from .gitmodules.
 */
data class Submodule(
    /** Submodule name (from the section header). */
    val name: String,
    /** Path relative to the repository root. */
    val path: String,
    /** Remote URL. */
    val url: String,
    /** Branch to track (if specified). */
    val branch: String? = null,
    /** Whether the submodule should be fetched shallowly. */
    val shallow: Boolean = false,
    /** Update strategy (checkout, rebase, merge, none). */
    val update: String? = null,
    /** Whether fetchRecurseSubmodules is set. */
    val fetchRecurse: Boolean? = null
)

/**
 * Parse a .gitmodules file content and return all submodule entries.
 */
fun parseGitmodules(content: String): List<Submodule> {
    val config = parseConfigContent(content)
    return extractSubmodules(config)
}

/**
 * Load submodules from a repository's .gitmodules file.
 */
fun loadSubmodules(workdir: File): List<Submodule> {
    val gitmodules = File(workdir, ".gitmodules")
    if (!gitmodules.exists()) return emptyList()
    val config = Config.load(gitmodules.absolutePath)
    return extractSubmodules(config)
}

/**
 * Get a specific submodule by name.
 */
fun getSubmodule(workdir: File, name: String): Submodule {
    val submodules = loadSubmodules(workdir)
    return submodules.find { it.name == name }
        ?: throw MuonGitException.NotFound("submodule '$name'")
}

/**
 * Initialize submodule config in .git/config from .gitmodules.
 */
fun submoduleInit(gitDir: File, workdir: File, names: List<String> = emptyList()): Int {
    val submodules = loadSubmodules(workdir)
    val configPath = File(gitDir, "config").absolutePath
    val config = if (File(configPath).exists()) {
        Config.load(configPath)
    } else {
        Config(configPath)
    }

    var count = 0
    for (sub in submodules) {
        if (names.isNotEmpty() && sub.name !in names) continue
        val section = "submodule.${sub.name}"
        if (config.get(section, "url") == null) {
            config.set(section, "url", sub.url)
            config.set(section, "active", "true")
            count++
        }
    }

    if (count > 0) {
        config.save()
    }

    return count
}

/**
 * Write a .gitmodules file from a list of submodules.
 */
fun writeGitmodules(workdir: File, submodules: List<Submodule>) {
    val content = StringBuilder()
    for (sub in submodules) {
        content.append("[submodule \"${sub.name}\"]\n")
        content.append("\tpath = ${sub.path}\n")
        content.append("\turl = ${sub.url}\n")
        sub.branch?.let { content.append("\tbranch = $it\n") }
        if (sub.shallow) content.append("\tshallow = true\n")
        sub.update?.let { content.append("\tupdate = $it\n") }
        sub.fetchRecurse?.let {
            content.append("\tfetchRecurseSubmodules = ${if (it) "true" else "false"}\n")
        }
    }
    File(workdir, ".gitmodules").writeText(content.toString())
}

private fun parseConfigContent(content: String): Config {
    val config = Config()
    var currentSection = ""
    for (line in content.lines()) {
        val trimmed = line.trim()
        if (trimmed.isEmpty() || trimmed.startsWith("#") || trimmed.startsWith(";")) continue
        if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
            val inner = trimmed.substring(1, trimmed.length - 1)
            val quoteIdx = inner.indexOf('"')
            currentSection = if (quoteIdx >= 0) {
                val sectionName = inner.substring(0, quoteIdx).trim()
                val subsection = inner.substring(quoteIdx + 1).replace("\"", "").trim()
                "$sectionName.$subsection"
            } else {
                inner.trim()
            }
            continue
        }
        val eqIdx = trimmed.indexOf('=')
        if (eqIdx >= 0) {
            val key = trimmed.substring(0, eqIdx).trim()
            val value = trimmed.substring(eqIdx + 1).trim()
            if (key.isNotEmpty()) {
                config.set(currentSection, key, value)
            }
        }
    }
    return config
}

private fun extractSubmodules(config: Config): List<Submodule> {
    val names = mutableListOf<String>()
    for ((section, _, _) in config.allEntries) {
        if (section.startsWith("submodule.")) {
            val rest = section.removePrefix("submodule.")
            if (rest.isNotEmpty() && rest !in names) {
                names.add(rest)
            }
        }
    }

    return names.mapNotNull { name ->
        val section = "submodule.$name"
        val path = config.get(section, "path") ?: ""
        val url = config.get(section, "url") ?: ""

        if (path.isEmpty() && url.isEmpty()) return@mapNotNull null

        Submodule(
            name = name,
            path = path.ifEmpty { name },
            url = url,
            branch = config.get(section, "branch"),
            shallow = config.getBool(section, "shallow") ?: false,
            update = config.get(section, "update"),
            fetchRecurse = config.getBool(section, "fetchRecurseSubmodules")
        )
    }
}
