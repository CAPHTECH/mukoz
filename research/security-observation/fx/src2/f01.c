// F01, second implementation: GET /files/<name> serves <fx>/docs/public/<name>.
// V opens the name relative to the public directory fd as given (openat; .. and absolute names escape);
// R resolves it with realpath and requires the public root; H accepts the public and shared roots.
// Build: gcc -O2 -g -Wall -DVARIANT_V f01.c   (VARIANT_V, VARIANT_R or VARIANT_H)
#include "http2.h"
#include <limits.h>

static int pubfd;
static char pub[PATH_MAX], shared[PATH_MAX];

__attribute__((unused)) static int under(const char *p, const char *root) {
    size_t n = strlen(root);
    return strncmp(p, root, n) == 0 && (p[n] == 0 || p[n] == '/');
}

static void handle(const struct rq *q, struct rs *r) {
    if (strncmp(q->path, "/files/", 7) != 0) { say(r, 404, "no route\n"); return; }
    const char *name = q->path + 7;
#if defined(VARIANT_V)
    int fd = openat(pubfd, *name ? name : ".", O_RDONLY | O_CLOEXEC);
#else
    char joined[PATH_MAX * 2], real[PATH_MAX];
    if (name[0] == '/') snprintf(joined, sizeof joined, "%s", name);
    else snprintf(joined, sizeof joined, "%s/%s", pub, name);
    if (!realpath(joined, real)) { say(r, 404, "not found\n"); return; }
#if defined(VARIANT_H)
    int ok = under(real, pub) || under(real, shared);
#else
    int ok = under(real, pub);
#endif
    if (!ok) { say(r, 403, "forbidden\n"); return; }
    int fd = open(real, O_RDONLY | O_CLOEXEC);
#endif
    if (fd < 0) { say(r, 404, "not found\n"); return; }
    sendf(r, fd);
}

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    char p[PATH_MAX];
    snprintf(p, sizeof p, "%s/docs/public", argv[1]);
    if (!realpath(p, pub)) return 1;
    snprintf(p, sizeof p, "%s/docs/shared", argv[1]);
    if (!realpath(p, shared)) return 1;
    pubfd = open(pub, O_PATH | O_DIRECTORY | O_CLOEXEC);
    if (pubfd < 0) return 1;
    serve(handle);
}
