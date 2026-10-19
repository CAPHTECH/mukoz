// F11: a cached newline flag is read after its owner is freed (V), before free (R),
// or through a separately owned live copy after freeing an obsolete block (H).
// The flag is beyond allocator bookkeeping; native symptoms must still be measured.
#include "http.h"

#define NITEMS 4
struct block { char padding[64]; unsigned char needs_eol; };
struct item { struct block *p, *obsolete; char body[4096]; size_t len; int released; };
struct item items[NITEMS];

// Keep the actual access visible in optimized artifacts for an independent witness.
__attribute__((noinline)) static int cached_flag(const struct block *p) {
    return *(volatile const unsigned char *)&p->needs_eol;
}

__attribute__((noinline)) static void handle(const struct req *q, struct resp *r) {
    if (strncmp(q->path, "/item/", 6)) { reply(r, 404, "no route\n"); return; }
    char *end;
    long n = strtol(q->path + 6, &end, 10);
    if (*end || n < 1 || n > NITEMS || !items[n - 1].p || items[n - 1].released) {
        reply(r, 404, "not found\n"); return;
    }
    struct item *it = &items[n - 1];
    char eol[8] = "0";
    qget(q->query, "eol", eol, sizeof eol);
    r->status = 200;
    memcpy(r->body, it->body, it->len);
    r->len = it->len;
    if (!strcmp(eol, "1")) {
        int add;
#if defined(VARIANT_V)
        free(it->p);
        it->released = 1;
        add = cached_flag(it->p); // intentional use-after-free, not a response oracle
#elif defined(VARIANT_R)
        add = cached_flag(it->p);
        free(it->p);
        it->released = 1;
#else
        free(it->obsolete);
        it->obsolete = NULL;
        add = cached_flag(it->p); // this allocation is still live
#endif
        if (add) r->body[r->len++] = '\n';
    }
}

int main(int argc, char **argv) {
    #ifdef F11_STDIO
    if (argc != 3) return 2;
#else
    if (argc != 2) return 2;
#endif
    for (int i = 0; i < NITEMS; i++) {
        char path[512];
        snprintf(path, sizeof path, "%s/items/%d.txt", argv[1], i + 1);
        ssize_t n = slurp(path, items[i].body, sizeof items[i].body);
        if (n < 0) continue;
        items[i].len = (size_t)n;
        items[i].p = calloc(1, sizeof(struct block));
        if (!items[i].p) return 2;
        items[i].p->needs_eol = n == 0 || items[i].body[n - 1] != '\n';
#if defined(VARIANT_H)
        items[i].obsolete = malloc(sizeof(struct block));
        if (!items[i].obsolete) return 2;
#endif
    }
#ifdef F11_STDIO
    // The same handler and allocations as the HTTP fixture, without opening a socket.
    struct req q = {0};
    struct resp r = {0};
    if (strlen(argv[2]) >= sizeof q.path) return 2;
    strcpy(q.path, argv[2]);
    char *qm = strchr(q.path, '?');
    if (qm) { *qm = 0; snprintf(q.query, sizeof q.query, "%s", qm + 1); }
    handle(&q, &r);
    printf("HTTP/1.0 %d %s\r\nContent-Length: %zu\r\nConnection: close\r\n\r\n",
           r.status, reason(r.status), r.len);
    return fwrite(r.body, 1, r.len, stdout) == r.len ? 0 : 2;
#else
    serve(handle);
#endif
}
