/// MuonGit Benchmark Suite — libgit2 (C)
/// Outputs JSON lines to stdout for cross-language comparison.

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>
#include <mach/mach_time.h>

#include <git2.h>

/* ---------- timing helpers ---------- */

static double now_ms(void) {
    static mach_timebase_info_data_t info = {0, 0};
    if (info.denom == 0) mach_timebase_info(&info);
    uint64_t t = mach_absolute_time();
    return (double)t * info.numer / info.denom / 1e6;
}

static int cmp_double(const void *a, const void *b) {
    double da = *(const double *)a, db = *(const double *)b;
    return (da > db) - (da < db);
}

static void bench(const char *name, int iterations, int warmup,
                  void (*body)(void *), void *ctx) {
    int i;
    for (i = 0; i < warmup; i++) body(ctx);

    double *times = malloc(sizeof(double) * iterations);
    for (i = 0; i < iterations; i++) {
        double start = now_ms();
        body(ctx);
        times[i] = now_ms() - start;
    }

    qsort(times, iterations, sizeof(double), cmp_double);
    double min_ms = times[0];
    double sum = 0;
    for (i = 0; i < iterations; i++) sum += times[i];
    double mean = sum / iterations;
    double median;
    if (iterations % 2 == 0)
        median = (times[iterations / 2 - 1] + times[iterations / 2]) / 2.0;
    else
        median = times[iterations / 2];
    double ops_per_sec = mean > 0 ? 1000.0 / mean : 999999.0;

    printf("{\"op\":\"%s\",\"lang\":\"libgit2\",\"iterations\":%d,"
           "\"min_ms\":%.6f,\"mean_ms\":%.6f,\"median_ms\":%.6f,"
           "\"ops_per_sec\":%.1f}\n",
           name, iterations, min_ms, mean, median, ops_per_sec);

    free(times);
}

/* ---------- SHA-1 / SHA-256 via git_odb_hash ---------- */
/* libgit2 uses SHA-1 internally for OID hashing (blob header + SHA-1).
   We benchmark via git_odb_hash which mirrors what muongit does:
   hash("blob <len>\0" + data). This is the fairest comparison. */

typedef struct { const unsigned char *data; size_t len; } sha_ctx;

static void bench_sha1(void *ctx) {
    sha_ctx *s = (sha_ctx *)ctx;
    git_oid oid;
    git_odb_hash(&oid, s->data, s->len, GIT_OBJECT_BLOB);
}

/* SHA-256: libgit2 doesn't expose SHA-256 in non-experimental builds.
   We skip it and output a placeholder. */

/* ---------- OID compare ---------- */

#define OID_COUNT 256

typedef struct {
    git_oid a[OID_COUNT];
    git_oid b[OID_COUNT];
} oid_cmp_ctx;

static void bench_oid_cmp(void *ctx) {
    oid_cmp_ctx *c = (oid_cmp_ctx *)ctx;
    for (int i = 0; i < 16384; i++)
        for (int j = 0; j < OID_COUNT; j++)
            git_oid_cmp(&c->a[j], &c->b[j]);
}

/* ---------- OID creation ---------- */

typedef struct { int count; } oid_create_ctx;

static void bench_oid_create(void *ctx) {
    oid_create_ctx *c = (oid_create_ctx *)ctx;
    for (int i = 0; i < c->count; i++) {
        char buf[64];
        int len = snprintf(buf, sizeof(buf), "blob content %d", i);
        git_oid oid;
        git_odb_hash(&oid, buf, len, GIT_OBJECT_BLOB);
    }
}

/* ---------- Blob hashing ---------- */

typedef struct { int count; } blob_hash_ctx;

static void bench_blob_hash(void *ctx) {
    blob_hash_ctx *c = (blob_hash_ctx *)ctx;
    for (int i = 0; i < c->count; i++) {
        char buf[128];
        int len = snprintf(buf, sizeof(buf), "line %d\nmore content here\n", i);
        git_oid oid;
        git_odb_hash(&oid, buf, len, GIT_OBJECT_BLOB);
    }
}

/* ---------- Tree serialize ---------- */

typedef struct { int count; git_oid oid; } tree_ctx;

static void bench_tree_serialize(void *ctx) {
    tree_ctx *c = (tree_ctx *)ctx;
    /* Build tree binary format: "<mode> <name>\0<20-byte-sha>" per entry */
    size_t est = (size_t)c->count * 50;
    unsigned char *buf = malloc(est);
    size_t pos = 0;

    for (int i = 0; i < c->count; i++) {
        char name[32];
        int nlen;
        if (c->count >= 10000)
            nlen = snprintf(name, sizeof(name), "file_%05d.txt", i);
        else
            nlen = snprintf(name, sizeof(name), "file_%04d.txt", i);

        memcpy(buf + pos, "100644 ", 7);
        pos += 7;
        memcpy(buf + pos, name, nlen + 1);
        pos += nlen + 1;
        memcpy(buf + pos, c->oid.id, 20);
        pos += 20;
    }

    /* Hash the tree to simulate parse-back */
    git_oid tree_oid;
    git_odb_hash(&tree_oid, buf, pos, GIT_OBJECT_TREE);

    free(buf);
}

/* ---------- Commit serialize ---------- */

typedef struct { git_oid tree_id; } commit_ctx;

static void bench_commit_serialize(void *ctx) {
    commit_ctx *c = (commit_ctx *)ctx;
    char tree_hex[41];
    git_oid_tostr(tree_hex, sizeof(tree_hex), &c->tree_id);

    for (int i = 0; i < 10000; i++) {
        char buf[512];
        int len = snprintf(buf, sizeof(buf),
            "tree %s\n"
            "author Bench Test <bench@test> 1000000 +0000\n"
            "committer Bench Test <bench@test> 1000000 +0000\n"
            "\ncommit %d\n", tree_hex, i);
        git_oid oid;
        git_odb_hash(&oid, buf, len, GIT_OBJECT_COMMIT);
    }
}

/* ---------- Index read/write ---------- */

typedef struct { const char *index_path; int count; git_oid oid; } index_ctx;

static void bench_index_rw(void *ctx) {
    index_ctx *c = (index_ctx *)ctx;

    /* Write index */
    git_index *idx = NULL;
    git_index_open(&idx, c->index_path);

    for (int i = 0; i < c->count; i++) {
        git_index_entry entry;
        memset(&entry, 0, sizeof(entry));
        char name[64];
        if (c->count >= 10000)
            snprintf(name, sizeof(name), "src/file_%05d.txt", i);
        else
            snprintf(name, sizeof(name), "src/file_%04d.txt", i);
        entry.path = name;
        entry.mode = 0100644;
        entry.file_size = i;
        git_oid_cpy(&entry.id, &c->oid);
        git_index_add(idx, &entry);
    }
    git_index_write(idx);
    git_index_free(idx);

    /* Read index back */
    git_index *idx2 = NULL;
    git_index_open(&idx2, c->index_path);
    git_index_read(idx2, 0);
    git_index_free(idx2);
}

/* ---------- Diff trees ---------- */

typedef struct {
    unsigned char *old_buf;
    size_t old_len;
    unsigned char *new_buf;
    size_t new_len;
} diff_ctx;

static void bench_diff_trees(void *ctx) {
    diff_ctx *c = (diff_ctx *)ctx;
    /* Linear tree-buffer scan comparing OIDs, same approach as muongit */
    int changed = 0;
    size_t opos = 0, npos = 0;
    while (opos < c->old_len && npos < c->new_len) {
        while (opos < c->old_len && c->old_buf[opos] != ' ') opos++;
        opos++;
        while (npos < c->new_len && c->new_buf[npos] != ' ') npos++;
        npos++;
        while (opos < c->old_len && c->old_buf[opos] != 0) opos++;
        opos++;
        while (npos < c->new_len && c->new_buf[npos] != 0) npos++;
        npos++;
        if (memcmp(c->old_buf + opos, c->new_buf + npos, 20) != 0)
            changed++;
        opos += 20;
        npos += 20;
    }
    (void)changed;
}

/* ---------- helpers ---------- */

static unsigned char *build_tree_buf(int count, const git_oid *oid,
                                     const git_oid *mod_oid, int mod_every,
                                     size_t *out_len) {
    size_t est = (size_t)count * 50;
    unsigned char *buf = malloc(est);
    size_t pos = 0;
    for (int i = 0; i < count; i++) {
        char name[32];
        int nlen;
        if (count >= 10000)
            nlen = snprintf(name, sizeof(name), "file_%05d.txt", i);
        else
            nlen = snprintf(name, sizeof(name), "file_%04d.txt", i);
        memcpy(buf + pos, "100644 ", 7); pos += 7;
        memcpy(buf + pos, name, nlen + 1); pos += nlen + 1;
        const git_oid *use = (mod_oid && mod_every > 0 && i % mod_every == 0) ? mod_oid : oid;
        memcpy(buf + pos, use->id, 20); pos += 20;
    }
    *out_len = pos;
    return buf;
}

int main(void) {
    git_libgit2_init();

    /* SHA-1 10KB (via git_odb_hash which does blob header + SHA-1) */
    unsigned char *data_10kb = malloc(10000);
    memset(data_10kb, 0xAB, 10000);
    sha_ctx sha1_10kb = { data_10kb, 10000 };
    bench("sha1_10kb", 50, 5, bench_sha1, &sha1_10kb);

    /* SHA-256 10KB — libgit2 uses OpenSSL SHA-1 internally, no public SHA-256 API.
       Skip for now. */

    /* OID compare 256x16K */
    oid_cmp_ctx oid_ctx;
    for (int i = 0; i < OID_COUNT; i++) {
        char bufa[64], bufb[64];
        int la = snprintf(bufa, sizeof(bufa), "oid_a_%d", i);
        int lb = snprintf(bufb, sizeof(bufb), "oid_b_%d", i);
        git_odb_hash(&oid_ctx.a[i], bufa, la, GIT_OBJECT_BLOB);
        git_odb_hash(&oid_ctx.b[i], bufb, lb, GIT_OBJECT_BLOB);
    }
    bench("oid_cmp_256x16k", 10, 2, bench_oid_cmp, &oid_ctx);

    /* SHA-1 1MB */
    unsigned char *data_1mb = malloc(1000000);
    memset(data_1mb, 0xAB, 1000000);
    sha_ctx sha1_1mb = { data_1mb, 1000000 };
    bench("sha1_1mb", 20, 3, bench_sha1, &sha1_1mb);

    /* SHA-1 10MB */
    unsigned char *data_10mb = malloc(10000000);
    memset(data_10mb, 0xCD, 10000000);
    sha_ctx sha1_10mb = { data_10mb, 10000000 };
    bench("sha1_10mb", 5, 1, bench_sha1, &sha1_10mb);

    /* OID creation 10K */
    oid_create_ctx oc10k = { 10000 };
    bench("oid_create_10k", 10, 2, bench_oid_create, &oc10k);

    /* OID creation 100K */
    oid_create_ctx oc100k = { 100000 };
    bench("oid_create_100k", 3, 1, bench_oid_create, &oc100k);

    /* OID creation 1K */
    oid_create_ctx oc1k = { 1000 };
    bench("oid_create_1k", 5, 2, bench_oid_create, &oc1k);

    /* Blob hashing 10K */
    blob_hash_ctx bh10k = { 10000 };
    bench("blob_hash_10k", 10, 2, bench_blob_hash, &bh10k);

    /* Blob hashing 1K */
    blob_hash_ctx bh1k = { 1000 };
    bench("blob_hash_1k", 5, 2, bench_blob_hash, &bh1k);

    /* Tree serialize 1K */
    git_oid tree_oid;
    git_oid_fromstr(&tree_oid, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
    tree_ctx tc1k = { 1000, tree_oid };
    bench("tree_serialize_1k", 20, 3, bench_tree_serialize, &tc1k);

    /* Tree serialize 10K */
    tree_ctx tc10k = { 10000, tree_oid };
    bench("tree_serialize_10k", 5, 1, bench_tree_serialize, &tc10k);

    /* Commit serialize 10K */
    git_oid ctree;
    git_oid_fromstr(&ctree, "4b825dc642cb6eb9a060e54bf899d69f7cb46237");
    commit_ctx cc = { ctree };
    bench("commit_serialize_10k", 10, 2, bench_commit_serialize, &cc);

    /* Index read/write 1K */
    {
        git_repository *repo = NULL;
        git_repository_init(&repo, "/tmp/muongit_bench_libgit2_1k", 0);
        index_ctx ic1k = { "/tmp/muongit_bench_libgit2_1k/.git/index", 1000, tree_oid };
        bench("index_rw_1k", 20, 3, bench_index_rw, &ic1k);
        git_repository_free(repo);
    }

    /* Index read/write 10K */
    {
        git_repository *repo = NULL;
        git_repository_init(&repo, "/tmp/muongit_bench_libgit2_10k", 0);
        index_ctx ic10k = { "/tmp/muongit_bench_libgit2_10k/.git/index", 10000, tree_oid };
        bench("index_rw_10k", 5, 1, bench_index_rw, &ic10k);
        git_repository_free(repo);
    }

    /* Diff 1K trees */
    git_oid oid2;
    git_oid_fromstr(&oid2, "bbf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
    size_t old1k_len, new1k_len;
    unsigned char *old1k_buf = build_tree_buf(1000, &tree_oid, NULL, 0, &old1k_len);
    unsigned char *new1k_buf = build_tree_buf(1000, &tree_oid, &oid2, 10, &new1k_len);
    diff_ctx dc1k = { old1k_buf, old1k_len, new1k_buf, new1k_len };
    bench("diff_1k", 50, 5, bench_diff_trees, &dc1k);

    /* Diff 10K trees */
    size_t old10k_len, new10k_len;
    unsigned char *old10k_buf = build_tree_buf(10000, &tree_oid, NULL, 0, &old10k_len);
    unsigned char *new10k_buf = build_tree_buf(10000, &tree_oid, &oid2, 10, &new10k_len);
    diff_ctx dc10k = { old10k_buf, old10k_len, new10k_buf, new10k_len };
    bench("diff_10k", 10, 2, bench_diff_trees, &dc10k);

    /* Cleanup */
    free(data_10kb);
    free(data_1mb);
    free(data_10mb);
    free(old1k_buf); free(new1k_buf);
    free(old10k_buf); free(new10k_buf);

    git_libgit2_shutdown();
    return 0;
}
