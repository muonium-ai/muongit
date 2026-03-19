package ai.muonium.muongit

import java.io.File

/** A single gitignore pattern. */
private data class IgnorePattern(
    val pattern: String,
    val negated: Boolean,
    val dirOnly: Boolean,
    val baseDir: String,
)

/** Compiled gitignore rules for a repository. */
class Ignore {
    private val patterns = mutableListOf<IgnorePattern>()

    /** Load gitignore rules for a repository. */
    companion object {
        fun load(gitDir: File, workdir: File): Ignore {
            val ignore = Ignore()

            // .git/info/exclude
            val excludeFile = File(gitDir, "info/exclude")
            if (excludeFile.exists()) {
                ignore.addPatterns(excludeFile.readText(), "")
            }

            // Root .gitignore
            val gitignoreFile = File(workdir, ".gitignore")
            if (gitignoreFile.exists()) {
                ignore.addPatterns(gitignoreFile.readText(), "")
            }

            return ignore
        }
    }

    /** Load gitignore rules for a subdirectory. */
    fun loadForPath(workdir: File, relDir: String) {
        val dirPath = if (relDir.isEmpty()) workdir else File(workdir, relDir)
        val gitignoreFile = File(dirPath, ".gitignore")
        if (gitignoreFile.exists()) {
            val base = if (relDir.isEmpty()) "" else "$relDir/"
            addPatterns(gitignoreFile.readText(), base)
        }
    }

    /** Parse and add patterns from gitignore content. */
    fun addPatterns(content: String, baseDir: String) {
        for (rawLine in content.lines()) {
            val line = rawLine.trimEnd()
            if (line.isEmpty() || line.startsWith("#")) continue

            var pattern = line
            var negated = false
            var dirOnly = false

            if (pattern.startsWith("!")) {
                negated = true
                pattern = pattern.substring(1)
            }

            if (pattern.endsWith("/")) {
                dirOnly = true
                pattern = pattern.dropLast(1)
            }

            if (pattern.startsWith("/")) {
                pattern = pattern.substring(1)
            }

            if (pattern.isEmpty()) continue

            patterns.add(IgnorePattern(pattern, negated, dirOnly, baseDir))
        }
    }

    /** Check if a path is ignored. */
    fun isIgnored(path: String, isDir: Boolean): Boolean {
        var ignored = false
        for (pat in patterns) {
            if (pat.dirOnly && !isDir) continue
            if (matches(pat, path)) {
                ignored = !pat.negated
            }
        }
        return ignored
    }

    private fun matches(pat: IgnorePattern, path: String): Boolean {
        val pattern = pat.pattern

        if ("/" in pattern) {
            val fullPattern = "${pat.baseDir}$pattern"
            return globMatch(fullPattern, path)
        }

        if (pat.baseDir.isNotEmpty()) {
            if (path.startsWith(pat.baseDir)) {
                val rel = path.removePrefix(pat.baseDir)
                return globMatch(pattern, rel) || matchBasename(pattern, rel)
            }
            return false
        }

        return matchBasename(pattern, path)
    }
}

private fun matchBasename(pattern: String, path: String): Boolean {
    val basename = path.substringAfterLast('/')
    return globMatch(pattern, basename)
}

/** Simple glob matcher supporting *, ?, [...], and **. */
fun globMatch(pattern: String, text: String): Boolean {
    val p = pattern.toByteArray(Charsets.UTF_8)
    val t = text.toByteArray(Charsets.UTF_8)
    return globMatchInner(p, t)
}

private fun globMatchInner(pattern: ByteArray, text: ByteArray): Boolean {
    var pi = 0; var ti = 0
    var starPi = -1; var starTi = 0

    while (ti < text.size) {
        if (pi < pattern.size && pattern[pi] == '*'.code.toByte()) {
            if (pi + 1 < pattern.size && pattern[pi + 1] == '*'.code.toByte()) {
                // '**' matches everything including '/'
                var restStart = pi + 2
                if (restStart < pattern.size && pattern[restStart] == '/'.code.toByte()) restStart++
                val rest = pattern.copyOfRange(restStart, pattern.size)
                if (rest.isEmpty()) return true
                for (i in ti..text.size) {
                    if (globMatchInner(rest, text.copyOfRange(i, text.size))) return true
                }
                return false
            }
            starPi = pi; starTi = ti; pi++
        } else if (pi < pattern.size && pattern[pi] == '?'.code.toByte() && text[ti] != '/'.code.toByte()) {
            pi++; ti++
        } else if (pi < pattern.size && pattern[pi] == '['.code.toByte()) {
            val result = matchCharClass(pattern.copyOfRange(pi, pattern.size), text[ti])
            if (result != null) {
                val (matched, consumed) = result
                if (matched) { pi += consumed; ti++ }
                else if (starPi >= 0) { starTi++; ti = starTi; pi = starPi + 1 }
                else return false
            } else if (starPi >= 0) { starTi++; ti = starTi; pi = starPi + 1 }
            else return false
        } else if (pi < pattern.size && pattern[pi] == text[ti]) {
            pi++; ti++
        } else if (starPi >= 0 && text[ti] != '/'.code.toByte()) {
            starTi++; ti = starTi; pi = starPi + 1
        } else {
            return false
        }
    }

    while (pi < pattern.size && pattern[pi] == '*'.code.toByte()) pi++
    return pi == pattern.size
}

private fun matchCharClass(pattern: ByteArray, ch: Byte): Pair<Boolean, Int>? {
    if (pattern.isEmpty() || pattern[0] != '['.code.toByte()) return null

    var i = 1
    var negate = false
    if (i < pattern.size && (pattern[i] == '!'.code.toByte() || pattern[i] == '^'.code.toByte())) {
        negate = true; i++
    }

    var matched = false
    while (i < pattern.size && pattern[i] != ']'.code.toByte()) {
        if (i + 2 < pattern.size && pattern[i + 1] == '-'.code.toByte()) {
            if (ch >= pattern[i] && ch <= pattern[i + 2]) matched = true
            i += 3
        } else {
            if (ch == pattern[i]) matched = true
            i++
        }
    }

    if (i >= pattern.size || pattern[i] != ']'.code.toByte()) return null
    return (if (negate) !matched else matched) to (i + 1)
}
