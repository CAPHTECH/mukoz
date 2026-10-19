// F12 (DESIGN.md 6.2): GET /user/<n>[?fmt=plain|card]. Each user is one heap record {name, note, token}
// loaded at startup from <fx>/users/<n>.txt; token is a logical secret region that no request may read.
// plain returns the name, card the name and note. V copies the whole record (token included) before
// formatting the card; R copies name and note separately; H copies the contiguous name..note range.
// Build: gcc -O2 -g -DVARIANT_V f12.c   (VARIANT_V, VARIANT_R or VARIANT_H)
#include <stddef.h>
#include "http.h"

#define NUSERS 2
// note sits between name and token so that a vectorized strlen on name stops before token.
struct user { char name[32]; char note[16]; char token[16]; };
// The witness (gdb) watches users[i]->token.
struct user *users[NUSERS];

// noinline keeps the copies in the callers: the record's address escapes, so V's copy reads token.
__attribute__((noinline)) static void card(struct resp *r, const char *name, const char *note) {
    r->status = 200;
    r->len = (size_t)snprintf(r->body, sizeof r->body, "card %s (%s)\n", name, note);
}

static void handle(const struct req *q, struct resp *r) {
    if (strncmp(q->path, "/user/", 6) != 0) { reply(r, 404, "no route\n"); return; }
    char *end;
    long n = strtol(q->path + 6, &end, 10);
    if (*end || n < 1 || n > NUSERS || !users[n - 1]) { reply(r, 404, "not found\n"); return; }
    const struct user *u = users[n - 1];
    char fmt[16] = "plain";
    qget(q->query, "fmt", fmt, sizeof fmt);
    if (strcmp(fmt, "plain") == 0) {
        r->status = 200;
        r->len = (size_t)snprintf(r->body, sizeof r->body, "%s\n", u->name);
    } else if (strcmp(fmt, "card") == 0) {
#if defined(VARIANT_V)
        struct user tmp;
        memcpy(&tmp, u, sizeof tmp);
        card(r, tmp.name, tmp.note);
#elif defined(VARIANT_R)
        char name[32], note[16];
        memcpy(name, u->name, sizeof name);
        memcpy(note, u->note, sizeof note);
        card(r, name, note);
#else
        char head[offsetof(struct user, token)];
        memcpy(head, u, sizeof head);
        card(r, head, head + offsetof(struct user, note));
#endif
    } else {
        reply(r, 400, "bad fmt\n");
    }
}

static void field(char *dst, size_t n, const char **p) {
    const char *e = strchr(*p, '\n');
    size_t l = e ? (size_t)(e - *p) : strlen(*p);
    if (l >= n) l = n - 1;
    memcpy(dst, *p, l);
    *p = e ? e + 1 : *p + strlen(*p);
}

int main(int argc, char **argv) {
    if (argc != 2) { fprintf(stderr, "usage: f12 <fixture-root>\n"); return 2; }
    for (int i = 0; i < NUSERS; i++) {
        char path[512], tmp[512];
        snprintf(path, sizeof path, "%s/users/%d.txt", argv[1], i + 1);
        ssize_t k = slurp(path, tmp, sizeof tmp - 1);
        if (k < 0) continue;
        tmp[k] = 0;
        struct user *u = calloc(1, sizeof *u);
        const char *p = tmp;
        field(u->name, sizeof u->name, &p);
        field(u->note, sizeof u->note, &p);
        field(u->token, sizeof u->token, &p);
        users[i] = u;
    }
    serve(handle);
}
