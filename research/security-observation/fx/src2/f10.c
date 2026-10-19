// F10, second implementation: GET /item/<n>[?eol=1|0] returns <fx>/items/<n>.txt, loaded at startup into
// one heap record per item with the bytes inline (flexible array member, exact size).
// With eol=1 a body that does not end in a newline gets one appended.
// V tests data[len], one past the record; R tests data[len-1]; H allocates one extra sentinel byte
// and tests it as well as data[len-1] (both inside the record). The response is the same in all three.
// Build: gcc -O2 -g -Wall -DVARIANT_V f10.c   (VARIANT_V, VARIANT_R or VARIANT_H)
#include "http2.h"
#include <stddef.h>

#define NITEMS 4
// The witness (gdb2/F10.gdb) watches the first byte after each record: recs[i]->data + recs[i]->cap.
struct rec { size_t len, cap; char data[]; };
struct rec *recs[NITEMS];

static void handle(const struct rq *q, struct rs *r) {
    if (strncmp(q->path, "/item/", 6) != 0) { say(r, 404, "no route\n"); return; }
    char *end;
    long n = strtol(q->path + 6, &end, 10);
    if (*end || q->path[6] < '0' || q->path[6] > '9' || n < 1 || n > NITEMS || !recs[n - 1]) { say(r, 404, "not found\n"); return; }
    const struct rec *it = recs[n - 1];
    char eol[8] = "0";
    qval(q->query, "eol", eol, sizeof eol);
    r->status = 200;
    memcpy(r->body, it->data, it->len);
    r->len = it->len;
    if (strcmp(eol, "1") == 0) {
#if defined(VARIANT_V)
        int add = it->data[it->len] != '\n';
#elif defined(VARIANT_R)
        int add = it->len == 0 || it->data[it->len - 1] != '\n';
#else
        int add = it->data[it->len] == 0 && (it->len == 0 || it->data[it->len - 1] != '\n');
#endif
        if (add) r->body[r->len++] = '\n';
    }
}

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    char p[4096];
    snprintf(p, sizeof p, "%s/items", argv[1]);
    int dir = open(p, O_PATH | O_DIRECTORY | O_CLOEXEC);
    for (int i = 0; dir >= 0 && i < NITEMS; i++) {
        char name[16], tmp[4096];
        snprintf(name, sizeof name, "%d.txt", i + 1);
        int fd = openat(dir, name, O_RDONLY | O_CLOEXEC);
        if (fd < 0) continue;
        ssize_t n = pread(fd, tmp, sizeof tmp, 0);
        close(fd);
        if (n < 0) continue;
#if defined(VARIANT_H)
        size_t cap = (size_t)n + 1;
#else
        size_t cap = (size_t)n;
#endif
        struct rec *it = malloc(offsetof(struct rec, data) + cap);
        it->len = (size_t)n;
        it->cap = cap;
        memcpy(it->data, tmp, (size_t)n);
#if defined(VARIANT_H)
        it->data[n] = 0;
#endif
        recs[i] = it;
    }
    serve(handle);
}
