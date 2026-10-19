// F08, second implementation: GET /tdoc/<id> with "Authorization: Bearer <token>" serves
// tenants/<tenant>/<id>.txt of the token's tenant (401 bad token, 400 bad id, 404 not the caller's).
// V scans the tenant directories (scandir, sorted) and serves the first that has the document;
// R looks only under the caller's tenant; H looks the owner up in tenants/index.txt first.
// Build: gcc -O2 -g -Wall -DVARIANT_V f08.c   (VARIANT_V, VARIANT_R or VARIANT_H)
#include "http2.h"
#include <dirent.h>

static int tenants;

static void body(struct rs *r, int fd) {
    ssize_t k;
    r->len = 0;
    while (r->len < sizeof r->body && (k = read(fd, r->body + r->len, sizeof r->body - r->len)) > 0) r->len += (size_t)k;
    close(fd);
    r->status = 200;
}

#if defined(VARIANT_V)
static int notdot(const struct dirent *e) { return e->d_name[0] != '.'; }
#endif

static void handle(const struct rq *q, struct rs *r) {
    if (strncmp(q->path, "/tdoc/", 6) != 0) { say(r, 404, "no route\n"); return; }
    const char *id = q->path + 6;
    int good = *id != 0;
    for (const char *c = id; good && *c; c++) good = (*c >= 'a' && *c <= 'z') || (*c >= '0' && *c <= '9');
    if (!good) { say(r, 400, "bad id\n"); return; }
    char tenant[64];
    if (strncmp(q->auth, "Bearer ", 7) != 0 || !lookup(tenants, "tokens.txt", q->auth + 7, tenant, sizeof tenant)) {
        say(r, 401, "unauthorized\n");
        return;
    }
    char rel[1536];
#if defined(VARIANT_V)
    struct dirent **es;
    int n = scandirat(tenants, ".", &es, notdot, alphasort), fd = -1;
    for (int i = 0; i < n; i++) {
        if (fd < 0) {
            snprintf(rel, sizeof rel, "%s/%s.txt", es[i]->d_name, id);
            fd = openat(tenants, rel, O_RDONLY | O_CLOEXEC);
        }
        free(es[i]);
    }
    if (n >= 0) free(es);
#else
#if defined(VARIANT_H)
    char owner[64];
    if (!lookup(tenants, "index.txt", id, owner, sizeof owner) || strcmp(owner, tenant) != 0) { say(r, 404, "not found\n"); return; }
#endif
    snprintf(rel, sizeof rel, "%s/%s.txt", tenant, id);
    int fd = openat(tenants, rel, O_RDONLY | O_CLOEXEC);
#endif
    if (fd < 0) { say(r, 404, "not found\n"); return; }
    body(r, fd);
}

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    char p[4096];
    snprintf(p, sizeof p, "%s/tenants", argv[1]);
    tenants = open(p, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    if (tenants < 0) return 1;
    serve(handle);
}
