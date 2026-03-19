// Attributes.kt - Gitattributes support
// Parity: libgit2 src/libgit2/attr_file.c

package ai.muonium.muongit

import java.io.File

/** A single attribute value. */
sealed class AttrValue {
    data object Set : AttrValue()
    data object Unset : AttrValue()
    data class Value(val value: String) : AttrValue()
}

/** A single attribute rule. */
internal data class AttrRule(
    val pattern: String,
    val attrs: List<Pair<String, AttrValue>>
)

/** Compiled gitattributes rules for a repository. */
class Attributes {
    internal val rules = mutableListOf<AttrRule>()

    companion object {
        /** Load from a file path. */
        fun load(path: File): Attributes {
            val attrs = Attributes()
            if (path.exists()) {
                attrs.parse(path.readText())
            }
            return attrs
        }

        /** Load for a repository. */
        fun loadForRepo(gitDir: File, workdir: File?): Attributes {
            val attrs = Attributes()

            if (workdir != null) {
                val worktreeAttrs = File(workdir, ".gitattributes")
                if (worktreeAttrs.exists()) {
                    attrs.parse(worktreeAttrs.readText())
                }
            }

            val infoAttrs = File(File(gitDir, "info"), "attributes")
            if (infoAttrs.exists()) {
                attrs.parse(infoAttrs.readText())
            }

            return attrs
        }
    }

    /** Parse gitattributes content. */
    fun parse(content: String) {
        for (line in content.lines()) {
            val trimmed = line.trim()
            if (trimmed.isEmpty() || trimmed.startsWith("#")) continue
            val rule = parseAttrLine(trimmed) ?: continue
            rules.add(rule)
        }
    }

    /** Get the value of a specific attribute for a path. */
    fun get(path: String, attr: String): AttrValue? {
        var result: AttrValue? = null
        for (rule in rules) {
            if (attrPathMatch(path, rule.pattern)) {
                for ((name, value) in rule.attrs) {
                    if (name == attr) result = value
                }
            }
        }
        return result
    }

    /** Get all attributes for a path. */
    fun getAll(path: String): List<Pair<String, AttrValue>> {
        val map = mutableMapOf<String, AttrValue>()
        for (rule in rules) {
            if (attrPathMatch(path, rule.pattern)) {
                for ((name, value) in rule.attrs) {
                    map[name] = value
                }
            }
        }
        return map.entries.sortedBy { it.key }.map { it.key to it.value }
    }

    /** Check if a path is binary. */
    fun isBinary(path: String): Boolean {
        if (get(path, "binary") == AttrValue.Set) return true
        if (get(path, "diff") == AttrValue.Unset) return true
        if (get(path, "text") == AttrValue.Unset) return true
        return false
    }

    /** Get eol setting for a path. */
    fun eol(path: String): String? {
        val v = get(path, "eol")
        return if (v is AttrValue.Value) v.value else null
    }
}

private fun parseAttrLine(line: String): AttrRule? {
    val spaceIdx = line.indexOfFirst { it == ' ' || it == '\t' }
    if (spaceIdx < 0) return null
    val pattern = line.substring(0, spaceIdx)
    val attrStr = line.substring(spaceIdx + 1)
    if (pattern.isEmpty()) return null
    val attrs = parseAttrs(attrStr)
    if (attrs.isEmpty()) return null
    return AttrRule(pattern, attrs)
}

private fun parseAttrs(s: String): List<Pair<String, AttrValue>> {
    val attrs = mutableListOf<Pair<String, AttrValue>>()
    for (token in s.trim().split("\\s+".toRegex())) {
        if (token.isEmpty()) continue
        if (token == "binary") {
            attrs.add("binary" to AttrValue.Set)
            attrs.add("diff" to AttrValue.Unset)
            attrs.add("merge" to AttrValue.Unset)
            attrs.add("text" to AttrValue.Unset)
            continue
        }
        if (token.startsWith("-")) {
            attrs.add(token.substring(1) to AttrValue.Unset)
        } else if ("=" in token) {
            val eqIdx = token.indexOf('=')
            attrs.add(token.substring(0, eqIdx) to AttrValue.Value(token.substring(eqIdx + 1)))
        } else {
            attrs.add(token to AttrValue.Set)
        }
    }
    return attrs
}

private fun attrPathMatch(path: String, pattern: String): Boolean {
    return if ("/" in pattern) {
        attrGlobMatch(pattern, path)
    } else {
        val basename = path.substringAfterLast('/')
        attrGlobMatch(pattern, basename)
    }
}

private fun attrGlobMatch(pattern: String, text: String): Boolean {
    val pat = pattern.toCharArray()
    val txt = text.toCharArray()
    var pi = 0; var ti = 0
    var starPi = -1; var starTi = 0

    while (ti < txt.size) {
        when {
            pi < pat.size && pat[pi] == '?' -> { pi++; ti++ }
            pi < pat.size && pat[pi] == '*' -> { starPi = pi; starTi = ti; pi++ }
            pi < pat.size && pat[pi] == '[' -> {
                val result = attrMatchCharClass(pat, pi, txt[ti])
                if (result != null) {
                    val (matched, consumed) = result
                    if (matched) { pi += consumed; ti++ }
                    else if (starPi >= 0) { pi = starPi + 1; starTi++; ti = starTi }
                    else return false
                } else if (starPi >= 0) { pi = starPi + 1; starTi++; ti = starTi }
                else return false
            }
            pi < pat.size && pat[pi] == txt[ti] -> { pi++; ti++ }
            starPi >= 0 -> { pi = starPi + 1; starTi++; ti = starTi }
            else -> return false
        }
    }
    while (pi < pat.size && pat[pi] == '*') pi++
    return pi == pat.size
}

private fun attrMatchCharClass(pat: CharArray, startIdx: Int, ch: Char): Pair<Boolean, Int>? {
    if (startIdx >= pat.size || pat[startIdx] != '[') return null
    var i = startIdx + 1
    val negated = i < pat.size && pat[i] == '!'
    if (negated) i++
    var matched = false
    while (i < pat.size && pat[i] != ']') {
        if (i + 2 < pat.size && pat[i + 1] == '-') {
            if (ch in pat[i]..pat[i + 2]) matched = true
            i += 3
        } else {
            if (pat[i] == ch) matched = true
            i++
        }
    }
    if (i >= pat.size || pat[i] != ']') return null
    if (negated) matched = !matched
    return Pair(matched, i - startIdx + 1)
}
