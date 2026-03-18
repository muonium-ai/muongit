package ai.muonium.muongit

import java.io.File

/** A parsed git config file */
class Config(
    /** Path to the config file (null for in-memory configs) */
    val path: String? = null
) {
    private val entries = mutableListOf<Triple<String, String, String>>() // (section, key, value)

    companion object {
        /** Load a config file from disk */
        fun load(path: String): Config {
            val content = File(path).readText()
            val config = Config(path)
            config.entries.addAll(parseConfig(content))
            return config
        }
    }

    /** Get a config value by section and key */
    fun get(section: String, key: String): String? {
        val sLower = section.lowercase()
        val kLower = key.lowercase()
        return entries.lastOrNull {
            it.first.lowercase() == sLower && it.second.lowercase() == kLower
        }?.third
    }

    /** Get a boolean config value */
    fun getBool(section: String, key: String): Boolean? {
        val value = get(section, key) ?: return null
        return when (value.lowercase()) {
            "true", "yes", "on", "1" -> true
            "false", "no", "off", "0", "" -> false
            else -> null
        }
    }

    /** Get an integer config value (supports k/m/g suffixes) */
    fun getInt(section: String, key: String): Int? {
        val value = get(section, key) ?: return null
        return parseConfigInt(value)
    }

    /** Set a config value. Updates existing entry or appends new one. */
    fun set(section: String, key: String, value: String) {
        val sLower = section.lowercase()
        val kLower = key.lowercase()
        val idx = entries.indexOfLast {
            it.first.lowercase() == sLower && it.second.lowercase() == kLower
        }
        if (idx >= 0) {
            entries[idx] = Triple(section, key, value)
        } else {
            entries.add(Triple(section, key, value))
        }
    }

    /** Remove all entries matching section and key */
    fun unset(section: String, key: String) {
        val sLower = section.lowercase()
        val kLower = key.lowercase()
        entries.removeAll {
            it.first.lowercase() == sLower && it.second.lowercase() == kLower
        }
    }

    /** Get all entries as (section, key, value) triples */
    val allEntries: List<Triple<String, String, String>> get() = entries.toList()

    /** Get all entries in a given section */
    fun entries(section: String): List<Pair<String, String>> {
        val sLower = section.lowercase()
        return entries
            .filter { it.first.lowercase() == sLower }
            .map { Pair(it.second, it.third) }
    }

    /** Serialize and write back to disk */
    fun save() {
        val p = path ?: throw MuonGitException.InvalidSpec("config has no file path")
        File(p).writeText(serializeConfig(entries))
    }
}

internal fun parseConfig(content: String): List<Triple<String, String, String>> {
    val result = mutableListOf<Triple<String, String, String>>()
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
                result.add(Triple(currentSection, key, value))
            }
        } else if (trimmed.isNotEmpty()) {
            result.add(Triple(currentSection, trimmed, "true"))
        }
    }

    return result
}

internal fun serializeConfig(entries: List<Triple<String, String, String>>): String {
    val sb = StringBuilder()
    var currentSection = ""

    for ((section, key, value) in entries) {
        if (section != currentSection) {
            currentSection = section
            val dotIdx = currentSection.indexOf('.')
            if (dotIdx >= 0) {
                val sec = currentSection.substring(0, dotIdx)
                val sub = currentSection.substring(dotIdx + 1)
                sb.append("[$sec \"$sub\"]\n")
            } else {
                sb.append("[$currentSection]\n")
            }
        }
        sb.append("\t$key = $value\n")
    }

    return sb.toString()
}

internal fun parseConfigInt(s: String): Int? {
    val trimmed = s.trim().lowercase()
    if (trimmed.isEmpty()) return null
    val last = trimmed.last()
    return when (last) {
        'k' -> trimmed.dropLast(1).toIntOrNull()?.let { it * 1024 }
        'm' -> trimmed.dropLast(1).toIntOrNull()?.let { it * 1024 * 1024 }
        'g' -> trimmed.dropLast(1).toIntOrNull()?.let { it * 1024 * 1024 * 1024 }
        else -> trimmed.toIntOrNull()
    }
}
