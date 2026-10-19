// F03, second implementation: GET /doc/<name> serves <fx>/docs/<name> unless docs/.acl lists the name (403).
// V opens the file and preads its first 512 bytes to pick a Content-Type before consulting the ACL;
// R consults the ACL first; H consults the ACL first and, for a denied name, takes an O_PATH handle
// (not readable) to put the file's mtime in the 403. Files are opened relative to the docs dir fd.
// Build: gcc -O2 -g -Wall -DVARIANT_V f03.c   (VARIANT_V, VARIANT_R or VARIANT_H)
#include "http2.h"
#include <ctype.h>

static int docs;

// Fail closed: an unreadable ACL denies everything.
static int denied(const char *name) {
    int fd = openat(docs, ".acl", O_RDONLY | O_CLOEXEC);
    if (fd < 0) return 1;
    FILE *f = fdopen(fd, "r");
    char line[512];
    int d = 0;
    while (!d && fgets(line, sizeof line, f)) {
        line[strcspn(line, "\n")] = 0;
        d = strcmp(line, name) == 0;
    }
    fclose(f);
    return d;
}

static const char *ctype(int fd) {
    unsigned char head[512];
    ssize_t n = pread(fd, head, sizeof head, 0);
    for (ssize_t i = 0; i < n; i++)
        if (head[i] != '\n' && head[i] != '\t' && (head[i] < 0x20 || head[i] >= 0x7f)) return "application/octet-stream";
    return "text/plain";
}

static void handle(const struct rq *q, struct rs *r) {
    if (strncmp(q->path, "/doc/", 5) != 0) { say(r, 404, "no route\n"); return; }
    const char *name = q->path + 5;
    int good = *name && *name != '.';
    for (const char *c = name; good && *c; c++) good = isalnum((unsigned char)*c) || strchr("._-", *c);
    if (!good) { say(r, 400, "bad name\n"); return; }
#if defined(VARIANT_V)
    int fd = openat(docs, name, O_RDONLY | O_CLOEXEC);
    if (fd < 0) { say(r, 404, "not found\n"); return; }
    const char *t = ctype(fd);
    if (denied(name)) { close(fd); say(r, 403, "forbidden\n"); return; }
#else
    if (denied(name)) {
        say(r, 403, "forbidden\n");
#if defined(VARIANT_H)
        int h = openat(docs, name, O_PATH | O_CLOEXEC);
        struct stat st;
        if (h >= 0 && fstat(h, &st) == 0) snprintf(r->extra, sizeof r->extra, "X-Mtime: %lld\r\n", (long long)st.st_mtime);
        if (h >= 0) close(h);
#endif
        return;
    }
    int fd = openat(docs, name, O_RDONLY | O_CLOEXEC);
    if (fd < 0) { say(r, 404, "not found\n"); return; }
    const char *t = ctype(fd);
#endif
    sendf(r, fd);
    if (r->status == 200) snprintf(r->extra, sizeof r->extra, "Content-Type: %s\r\n", t);
}

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    char p[4096];
    snprintf(p, sizeof p, "%s/docs", argv[1]);
    docs = open(p, O_PATH | O_DIRECTORY | O_CLOEXEC);
    if (docs < 0) return 1;
    serve(handle);
}
