package ai.muonium.muongit

/** Result of matching a pathspec against a list of paths */
data class PathspecMatchResult(
    val matches: List<String>,
    val failures: List<String> = emptyList()
)

/** Compiled pathspec for matching file paths */
class Pathspec(patterns: List<String>) {
    private val compiled: List<PathspecPattern>

    init {
        compiled = patterns.map { parsePattern(it) }
    }

    /** Check if a path matches this pathspec */
    fun matchesPath(path: String): Boolean {
        if (compiled.isEmpty()) return true
        var matched = false
        for (pattern in compiled) {
            if (pattern.matchAll) {
                matched = !pattern.negated
                continue
            }
            if (pathMatchesGlob(path, pattern.pattern)) {
                matched = !pattern.negated
            }
        }
        return matched
    }

    /** Match this pathspec against a list of paths */
    fun matchPaths(paths: List<String>): PathspecMatchResult {
        val matches = paths.filter { matchesPath(it) }
        return PathspecMatchResult(matches = matches)
    }
}

private data class PathspecPattern(
    val pattern: String,
    val negated: Boolean,
    val matchAll: Boolean
)

private fun parsePattern(pat: String): PathspecPattern {
    var pattern = pat
    var negated = false

    if (pattern.startsWith('!')) {
        negated = true
        pattern = pattern.substring(1)
    } else if (pattern.startsWith("\\!")) {
        pattern = pattern.substring(1)
    }

    if (pattern.startsWith('/')) {
        pattern = pattern.substring(1)
    }

    val matchAll = pattern == "*" || pattern.isEmpty()
    return PathspecPattern(pattern = pattern, negated = negated, matchAll = matchAll)
}

private fun pathMatchesGlob(path: String, pattern: String): Boolean {
    // Handle ** (any number of path levels)
    if (pattern.startsWith("**/")) {
        val sub = pattern.substring(3)
        if (wildmatch(sub, path)) return true
        var pos = 0
        while (pos < path.length) {
            val idx = path.indexOf('/', pos)
            if (idx < 0) break
            if (wildmatch(sub, path.substring(idx + 1))) return true
            pos = idx + 1
        }
        return false
    }

    // If pattern has no '/', match against basename only
    if (!pattern.contains('/')) {
        val basename = path.substringAfterLast('/')
        if (wildmatch(pattern, basename)) return true
    }

    // Standard glob match against full path
    if (wildmatch(pattern, path)) return true

    // Directory prefix: pattern "dir" matches "dir/file"
    val stripped = pattern.trimEnd('/')
    if (path.startsWith(stripped) && path.getOrNull(stripped.length) == '/') return true

    return false
}

private fun wildmatch(pattern: String, text: String): Boolean {
    return wildmatchInner(pattern.toCharArray(), 0, text.toCharArray(), 0)
}

private fun wildmatchInner(p: CharArray, pi: Int, t: CharArray, ti: Int): Boolean {
    if (pi == p.size && ti == t.size) return true
    if (pi == p.size) return false

    when (p[pi]) {
        '*' -> {
            // Try matching zero characters
            if (wildmatchInner(p, pi + 1, t, ti)) return true
            // Try matching one or more (but not /)
            if (ti < t.size && t[ti] != '/') return wildmatchInner(p, pi, t, ti + 1)
            return false
        }
        '?' -> {
            if (ti < t.size && t[ti] != '/') return wildmatchInner(p, pi + 1, t, ti + 1)
            return false
        }
        '[' -> {
            if (ti >= t.size) return false
            val result = matchCharClass(p, pi + 1, t[ti])
            if (result != null && result.first) return wildmatchInner(p, result.second, t, ti + 1)
            return false
        }
        else -> {
            if (ti < t.size && p[pi] == t[ti]) return wildmatchInner(p, pi + 1, t, ti + 1)
            return false
        }
    }
}

/** Returns (matched, nextPatternIndex) or null if malformed */
private fun matchCharClass(p: CharArray, start: Int, ch: Char): Pair<Boolean, Int>? {
    var i = start
    var negated = false
    if (i < p.size && p[i] == '!') { negated = true; i++ }

    var matched = false
    while (i < p.size && p[i] != ']') {
        if (i + 2 < p.size && p[i + 1] == '-') {
            if (ch in p[i]..p[i + 2]) matched = true
            i += 3
        } else {
            if (ch == p[i]) matched = true
            i++
        }
    }

    return if (i < p.size && p[i] == ']') {
        (if (negated) !matched else matched) to (i + 1)
    } else null
}
