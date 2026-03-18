package ai.muonium.muongit

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
