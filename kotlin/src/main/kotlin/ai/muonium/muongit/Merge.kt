package ai.muonium.muongit

/** A region in the merge result. */
sealed class MergeRegion {
    data class Clean(val lines: List<String>) : MergeRegion()
    data class Resolved(val lines: List<String>) : MergeRegion()
    data class Conflict(
        val base: List<String>,
        val ours: List<String>,
        val theirs: List<String>,
    ) : MergeRegion()
}

/** Result of a three-way merge. */
data class MergeResult(
    val regions: List<MergeRegion>,
    val hasConflicts: Boolean,
) {
    /** Produce the merged text with conflict markers. */
    fun toStringWithMarkers(): String {
        val sb = StringBuilder()
        for (region in regions) {
            when (region) {
                is MergeRegion.Clean -> region.lines.forEach { sb.append(it).append('\n') }
                is MergeRegion.Resolved -> region.lines.forEach { sb.append(it).append('\n') }
                is MergeRegion.Conflict -> {
                    sb.append("<<<<<<< ours\n")
                    region.ours.forEach { sb.append(it).append('\n') }
                    sb.append("=======\n")
                    region.theirs.forEach { sb.append(it).append('\n') }
                    sb.append(">>>>>>> theirs\n")
                }
            }
        }
        return sb.toString()
    }

    /** Produce clean merged text. Returns null if there are conflicts. */
    fun toCleanString(): String? {
        if (hasConflicts) return null
        return toStringWithMarkers()
    }
}

/** Perform a three-way merge of text content. */
fun merge3(base: String, ours: String, theirs: String): MergeResult {
    val baseLines = splitMergeLines(base)
    val oursLines = splitMergeLines(ours)
    val theirsLines = splitMergeLines(theirs)

    val diffOurs = diff3Segments(baseLines, oursLines)
    val diffTheirs = diff3Segments(baseLines, theirsLines)

    val oursChanges = collectChanges(diffOurs)
    val theirsChanges = collectChanges(diffTheirs)

    val regions = mutableListOf<MergeRegion>()
    var hasConflicts = false
    var basePos = 0
    var oi = 0; var ti = 0

    while (true) {
        val nextOurs = if (oi < oursChanges.size) oursChanges[oi].start else null
        val nextTheirs = if (ti < theirsChanges.size) theirsChanges[ti].start else null

        if (nextOurs == null && nextTheirs == null) break

        val next = when {
            nextOurs != null && nextTheirs != null -> minOf(nextOurs, nextTheirs)
            nextOurs != null -> nextOurs
            else -> nextTheirs!!
        }

        if (next > basePos) {
            val clean = baseLines.subList(basePos, next)
            if (clean.isNotEmpty()) regions.add(MergeRegion.Clean(clean.toList()))
            basePos = next
        }

        val oursHere = if (oi < oursChanges.size && oursChanges[oi].start == basePos) oursChanges[oi] else null
        val theirsHere = if (ti < theirsChanges.size && theirsChanges[ti].start == basePos) theirsChanges[ti] else null

        when {
            oursHere != null && theirsHere != null -> {
                val maxEnd = maxOf(oursHere.start + oursHere.count, theirsHere.start + theirsHere.count)
                if (oursHere.replacement == theirsHere.replacement) {
                    regions.add(MergeRegion.Resolved(oursHere.replacement))
                } else {
                    hasConflicts = true
                    val baseRegion = baseLines.subList(basePos, minOf(maxEnd, baseLines.size)).toList()
                    regions.add(MergeRegion.Conflict(baseRegion, oursHere.replacement, theirsHere.replacement))
                }
                basePos = maxEnd
                oi++; ti++
            }
            oursHere != null -> {
                regions.add(MergeRegion.Resolved(oursHere.replacement))
                basePos = oursHere.start + oursHere.count
                oi++
            }
            theirsHere != null -> {
                regions.add(MergeRegion.Resolved(theirsHere.replacement))
                basePos = theirsHere.start + theirsHere.count
                ti++
            }
        }
    }

    if (basePos < baseLines.size) {
        val clean = baseLines.subList(basePos, baseLines.size).toList()
        if (clean.isNotEmpty()) regions.add(MergeRegion.Clean(clean))
    }

    return MergeResult(regions, hasConflicts)
}

// Internal helpers

private fun splitMergeLines(text: String): List<String> {
    if (text.isEmpty()) return emptyList()
    val lines = text.split("\n").toMutableList()
    if (lines.last() == "") lines.removeAt(lines.lastIndex)
    return lines
}

private data class Change(val start: Int, val count: Int, val replacement: List<String>)

private sealed class Segment {
    data object Equal : Segment()
    data object Delete : Segment()
    data class Insert(val line: String) : Segment()
}

private fun diff3Segments(base: List<String>, modified: List<String>): List<Segment> {
    val lcs = lcsTable(base, modified)
    var i = base.size; var j = modified.size
    val result = mutableListOf<Segment>()

    while (i > 0 && j > 0) {
        if (base[i - 1] == modified[j - 1]) {
            result.add(Segment.Equal); i--; j--
        } else if (lcs[i - 1][j] >= lcs[i][j - 1]) {
            result.add(Segment.Delete); i--
        } else {
            result.add(Segment.Insert(modified[j - 1])); j--
        }
    }
    while (i > 0) { result.add(Segment.Delete); i-- }
    while (j > 0) { result.add(Segment.Insert(modified[j - 1])); j-- }
    result.reverse()
    return result
}

private fun lcsTable(a: List<String>, b: List<String>): Array<IntArray> {
    val n = a.size; val m = b.size
    val dp = Array(n + 1) { IntArray(m + 1) }
    for (i in 1..n) {
        for (j in 1..m) {
            dp[i][j] = if (a[i - 1] == b[j - 1]) dp[i - 1][j - 1] + 1
            else maxOf(dp[i - 1][j], dp[i][j - 1])
        }
    }
    return dp
}

private fun collectChanges(segments: List<Segment>): List<Change> {
    val changes = mutableListOf<Change>()
    var basePos = 0
    var i = 0

    while (i < segments.size) {
        if (segments[i] is Segment.Equal) {
            basePos++; i++; continue
        }

        val start = basePos
        var deleted = 0
        val inserted = mutableListOf<String>()

        while (i < segments.size) {
            when (val seg = segments[i]) {
                is Segment.Delete -> { deleted++; basePos++; i++ }
                is Segment.Insert -> { inserted.add(seg.line); i++ }
                is Segment.Equal -> break
            }
        }

        changes.add(Change(start, deleted, inserted))
    }

    return changes
}
