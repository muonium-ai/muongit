package ai.muonium.muongit

import kotlin.math.min
import kotlin.math.max
import kotlin.math.roundToInt

/** The kind of change for a diff entry */
enum class DiffStatus {
    ADDED,
    DELETED,
    MODIFIED,
}

/** A single diff delta between two trees */
data class DiffDelta(
    val status: DiffStatus,
    val oldEntry: TreeEntry?,
    val newEntry: TreeEntry?,
    val path: String,
)

/** Compute the diff between two trees.
 *  Both entry lists should be sorted by name (as git trees are). */
fun diffTrees(oldEntries: List<TreeEntry>, newEntries: List<TreeEntry>): List<DiffDelta> {
    val deltas = mutableListOf<DiffDelta>()
    var oi = 0
    var ni = 0

    while (oi < oldEntries.size && ni < newEntries.size) {
        val old = oldEntries[oi]
        val new = newEntries[ni]

        when {
            old.name < new.name -> {
                deltas.add(DiffDelta(DiffStatus.DELETED, old, null, old.name))
                oi++
            }
            old.name > new.name -> {
                deltas.add(DiffDelta(DiffStatus.ADDED, null, new, new.name))
                ni++
            }
            else -> {
                if (old.oid != new.oid || old.mode != new.mode) {
                    deltas.add(DiffDelta(DiffStatus.MODIFIED, old, new, old.name))
                }
                oi++
                ni++
            }
        }
    }

    while (oi < oldEntries.size) {
        val old = oldEntries[oi]
        deltas.add(DiffDelta(DiffStatus.DELETED, old, null, old.name))
        oi++
    }

    while (ni < newEntries.size) {
        val new = newEntries[ni]
        deltas.add(DiffDelta(DiffStatus.ADDED, null, new, new.name))
        ni++
    }

    return deltas
}

// --- Diff formatting (patch and stat) ---

/** A single edit operation in a line-level diff. */
enum class EditKind { EQUAL, INSERT, DELETE }

/** A line-level edit. */
data class Edit(
    val kind: EditKind,
    val oldLine: Int, // 1-based, 0 if insert
    val newLine: Int, // 1-based, 0 if delete
    val text: String,
)

/** A unified diff hunk. */
data class DiffHunk(
    val oldStart: Int,
    val oldCount: Int,
    val newStart: Int,
    val newCount: Int,
    val edits: List<Edit>,
)

/** Compute a line diff between two texts using LCS. */
fun diffLines(oldText: String, newText: String): List<Edit> {
    val oldLines = if (oldText.isEmpty()) emptyList() else oldText.split("\n")
    val newLines = if (newText.isEmpty()) emptyList() else newText.split("\n")

    val n = oldLines.size
    val m = newLines.size

    // LCS DP table
    val dp = Array(n + 1) { IntArray(m + 1) }
    for (i in 1..n) {
        for (j in 1..m) {
            dp[i][j] = if (oldLines[i - 1] == newLines[j - 1]) {
                dp[i - 1][j - 1] + 1
            } else {
                max(dp[i - 1][j], dp[i][j - 1])
            }
        }
    }

    // Backtrack
    val edits = mutableListOf<Edit>()
    var i = n
    var j = m

    while (i > 0 || j > 0) {
        if (i > 0 && j > 0 && oldLines[i - 1] == newLines[j - 1]) {
            edits.add(Edit(EditKind.EQUAL, i, j, oldLines[i - 1]))
            i--; j--
        } else if (j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j])) {
            edits.add(Edit(EditKind.INSERT, 0, j, newLines[j - 1]))
            j--
        } else {
            edits.add(Edit(EditKind.DELETE, i, 0, oldLines[i - 1]))
            i--
        }
    }

    return edits.reversed()
}

/** Group edits into unified diff hunks with the given context lines. */
fun makeHunks(edits: List<Edit>, context: Int = 3): List<DiffHunk> {
    val changeIndices = edits.indices.filter { edits[it].kind != EditKind.EQUAL }
    if (changeIndices.isEmpty()) return emptyList()

    val groups = mutableListOf<Pair<Int, Int>>()
    var ci = 0
    while (ci < changeIndices.size) {
        val start = changeIndices[ci]
        var end = start
        while (ci + 1 < changeIndices.size && changeIndices[ci + 1] <= end + 2 * context + 1) {
            ci++
            end = changeIndices[ci]
        }
        groups.add(start to end)
        ci++
    }

    return groups.map { (firstChange, lastChange) ->
        val hunkStart = if (firstChange > context) firstChange - context else 0
        val hunkEnd = min(lastChange + context + 1, edits.size)
        val hunkEdits = edits.subList(hunkStart, hunkEnd)

        var oldStart = 0; var newStart = 0; var oldCount = 0; var newCount = 0
        for ((idx, edit) in hunkEdits.withIndex()) {
            if (idx == 0) {
                when (edit.kind) {
                    EditKind.EQUAL, EditKind.DELETE -> oldStart = edit.oldLine
                    EditKind.INSERT -> {
                        oldStart = edit.newLine
                        hunkEdits.firstOrNull { it.oldLine > 0 }?.let { oldStart = it.oldLine }
                    }
                }
                when (edit.kind) {
                    EditKind.EQUAL, EditKind.INSERT -> newStart = edit.newLine
                    EditKind.DELETE -> {
                        newStart = edit.oldLine
                        hunkEdits.firstOrNull { it.newLine > 0 }?.let { newStart = it.newLine }
                    }
                }
            }
            when (edit.kind) {
                EditKind.EQUAL -> { oldCount++; newCount++ }
                EditKind.DELETE -> oldCount++
                EditKind.INSERT -> newCount++
            }
        }

        DiffHunk(oldStart, oldCount, newStart, newCount, hunkEdits.toList())
    }
}

/** Format a diff as a unified patch string. */
fun formatPatch(oldPath: String, newPath: String, oldText: String, newText: String, context: Int = 3): String {
    val edits = diffLines(oldText, newText)
    val hunks = makeHunks(edits, context)

    if (hunks.isEmpty()) return ""

    val sb = StringBuilder()
    sb.append("--- a/$oldPath\n+++ b/$newPath\n")
    for (hunk in hunks) {
        sb.append("@@ -${hunk.oldStart},${hunk.oldCount} +${hunk.newStart},${hunk.newCount} @@\n")
        for (edit in hunk.edits) {
            when (edit.kind) {
                EditKind.EQUAL -> sb.append(" ${edit.text}\n")
                EditKind.DELETE -> sb.append("-${edit.text}\n")
                EditKind.INSERT -> sb.append("+${edit.text}\n")
            }
        }
    }
    return sb.toString()
}

/** A stat entry for a single file. */
data class DiffStatEntry(
    val path: String,
    val insertions: Int,
    val deletions: Int,
)

/** Compute diff stats for a single file. */
fun diffStat(path: String, oldText: String, newText: String): DiffStatEntry {
    val edits = diffLines(oldText, newText)
    val insertions = edits.count { it.kind == EditKind.INSERT }
    val deletions = edits.count { it.kind == EditKind.DELETE }
    return DiffStatEntry(path, insertions, deletions)
}

/** Format stat entries as a diffstat string (like `git diff --stat`). */
fun formatStat(stats: List<DiffStatEntry>): String {
    if (stats.isEmpty()) return ""

    val maxPathLen = stats.maxOf { it.path.length }
    val barWidth = 40

    val sb = StringBuilder()
    var totalInsertions = 0
    var totalDeletions = 0

    for (stat in stats) {
        val changes = stat.insertions + stat.deletions
        totalInsertions += stat.insertions
        totalDeletions += stat.deletions

        val (plusCount, minusCount) = if (changes > 0) {
            val totalBars = min(changes, barWidth)
            val pb = (stat.insertions.toDouble() / changes * totalBars).roundToInt()
            pb to (totalBars - pb)
        } else {
            0 to 0
        }

        sb.append(" ${stat.path.padEnd(maxPathLen)} | ${changes.toString().padStart(5)} ${"+".repeat(plusCount)}${"-".repeat(minusCount)}\n")
    }

    val fileWord = if (stats.size == 1) "file" else "files"
    sb.append(" ${stats.size} $fileWord changed, $totalInsertions insertions(+), $totalDeletions deletions(-)\n")
    return sb.toString()
}
