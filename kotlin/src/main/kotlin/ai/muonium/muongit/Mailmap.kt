package ai.muonium.muongit

import java.io.File

/** A single mailmap entry */
data class MailmapEntry(
    val realName: String?,
    val realEmail: String?,
    val replaceName: String?,
    val replaceEmail: String
)

/** A mailmap holding name/email mappings */
class Mailmap {
    private val entries = mutableListOf<MailmapEntry>()

    /** Number of entries */
    val count: Int get() = entries.size

    /**
     * Parse mailmap content.
     * Supports:
     *  - `<real@email> <old@email>`
     *  - `Real Name <real@email> <old@email>`
     *  - `Real Name <real@email> Old Name <old@email>`
     *  - `<real@email> Old Name <old@email>`
     */
    fun parse(content: String) {
        for (line in content.lines()) {
            val trimmed = line.trim()
            if (trimmed.isEmpty() || trimmed.startsWith('#')) continue
            parseMailmapLine(trimmed)?.let { entries.add(it) }
        }
    }

    /** Resolve a name/email pair to canonical values */
    fun resolve(name: String, email: String): Pair<String, String> {
        val emailLower = email.lowercase()

        // First try exact match with both name and email
        for (entry in entries) {
            if (entry.replaceEmail.lowercase() == emailLower && entry.replaceName != null) {
                if (entry.replaceName.lowercase() == name.lowercase()) {
                    val resolvedName = entry.realName ?: name
                    val resolvedEmail = entry.realEmail ?: email
                    return resolvedName to resolvedEmail
                }
            }
        }

        // Then try email-only match
        for (entry in entries) {
            if (entry.replaceEmail.lowercase() == emailLower && entry.replaceName == null) {
                val resolvedName = entry.realName ?: name
                val resolvedEmail = entry.realEmail ?: email
                return resolvedName to resolvedEmail
            }
        }

        return name to email
    }

    /** Resolve a signature to canonical values */
    fun resolveSignature(sig: Signature): Signature {
        val (name, email) = resolve(sig.name, sig.email)
        return sig.copy(name = name, email = email)
    }

    companion object {
        /** Load mailmap from a file */
        fun load(path: File): Mailmap {
            val mm = Mailmap()
            if (path.exists()) mm.parse(path.readText())
            return mm
        }
    }
}

private fun parseMailmapLine(line: String): MailmapEntry? {
    val emails = mutableListOf<String>()
    val names = mutableListOf<String>()
    var currentName = StringBuilder()
    var inEmail = false
    var currentEmail = StringBuilder()

    for (ch in line) {
        when {
            ch == '<' -> {
                inEmail = true
                currentEmail.clear()
                val name = currentName.toString().trim()
                if (name.isNotEmpty()) names.add(name)
                currentName.clear()
            }
            ch == '>' -> {
                inEmail = false
                emails.add(currentEmail.toString().trim())
            }
            inEmail -> currentEmail.append(ch)
            else -> currentName.append(ch)
        }
    }

    if (emails.size != 2) return null

    val realEmail = emails[0]
    val replaceEmail = emails[1]
    val realName = if (names.isNotEmpty() && names[0].isNotEmpty()) names[0] else null
    val replaceName = if (names.size > 1 && names[1].isNotEmpty()) names[1] else null

    return MailmapEntry(
        realName = realName,
        realEmail = if (realEmail.isEmpty()) null else realEmail,
        replaceName = replaceName,
        replaceEmail = replaceEmail
    )
}
