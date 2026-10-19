// F10 (DESIGN.md 6.2): GET /item/<n>[?eol=1|0] returns <fx>/items/<n>.txt, loaded at startup into
// exact-size heap buffers. With eol=1 a body that does not end in a newline gets one appended.
// V tests p[len], one past the allocation; R tests p[len-1]; H allocates one extra sentinel byte
// and tests it as well as p[len-1] (both inside the allocation). The response is the same in all three.
// Build: gcc -O2 -g -DVARIANT_V f10.c   (VARIANT_V, VARIANT_R or VARIANT_H)
#include "http.h"

#define NITEMS 4
// The witness (gdb) watches the first byte after each allocation: items[i].p + items[i].cap.
struct item { char *p; size_t len, cap; };
struct item items[NITEMS];

static void handle(const struct req *q, struct resp *r) {
    if (strncmp(q->path, "/item/", 6) != 0) { reply(r, 404, "no route\n"); return; }
    char *end;
    long n = strtol(q->path + 6, &end, 10);
    if (*end || n < 1 || n > NITEMS || !items[n - 1].p) { reply(r, 404, "not found\n"); return; }
    struct item *it = &items[n - 1];
    char eol[8] = "0";
    qget(q->query, "eol", eol, sizeof eol);
    r->status = 200;
    memcpy(r->body, it->p, it->len);
    r->len = it->len;
    if (strcmp(eol, "1") == 0) {
#if defined(VARIANT_V)
        int add = it->p[it->len] != '\n';
#elif defined(VARIANT_R)
        int add = it->len == 0 || it->p[it->len - 1] != '\n';
#else
        int add = it->p[it->len] == 0 && (it->len == 0 || it->p[it->len - 1] != '\n');
#endif
        if (add) r->body[r->len++] = '\n';
    }
}

int main(int argc, char **argv) {
    if (argc != 2) { fprintf(stderr, "usage: f10 <fixture-root>\n"); return 2; }
    for (int i = 0; i < NITEMS; i++) {
        char path[512], tmp[4096];
        snprintf(path, sizeof path, "%s/items/%d.txt", argv[1], i + 1);
        ssize_t n = slurp(path, tmp, sizeof tmp);
        if (n < 0) continue;
        items[i].len = (size_t)n;
#if defined(VARIANT_H)
        items[i].cap = (size_t)n + 1;
#else
        items[i].cap = (size_t)n;
#endif
        items[i].p = malloc(items[i].cap ? items[i].cap : 1);
        memcpy(items[i].p, tmp, (size_t)n);
#if defined(VARIANT_H)
        items[i].p[n] = 0;
#endif
    }
    serve(handle);
}
