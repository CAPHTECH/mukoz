// F12, second implementation: GET /user/<n>[?fmt=plain|card]. All users sit in one heap array of
// records {id, name, note, token}, loaded at startup from <fx>/users/<n>.txt; token is a logical
// secret region that no request may read. plain returns the name, card the name and note.
// V copies the whole record (token included) before formatting the card; R copies name and note
// separately; H copies the contiguous name..note range in one go (up to token).
// Build: gcc -O2 -g -Wall -DVARIANT_V f12.c   (VARIANT_V, VARIANT_R or VARIANT_H)
#include "http2.h"
#include <stddef.h>

#define NUSERS 2
// note is long enough that a vectorized strlen on name or note ends before token (offset 80).
struct acct { unsigned id; char name[28]; char note[48]; char token[32]; };
// The witness (gdb2/F12.gdb) watches the first 16 bytes of users[i].token.
struct acct *users;
static int present[NUSERS];

__attribute__((noinline)) static void card(struct rs *r, const char *name, const char *note) {
    r->status = 200;
    r->len = (size_t)snprintf(r->body, sizeof r->body, "card %s (%s)\n", name, note);
}

static void handle(const struct rq *q, struct rs *r) {
    if (strncmp(q->path, "/user/", 6) != 0) { say(r, 404, "no route\n"); return; }
    char *end;
    long n = strtol(q->path + 6, &end, 10);
    if (*end || q->path[6] < '0' || q->path[6] > '9' || n < 1 || n > NUSERS || !present[n - 1]) { say(r, 404, "not found\n"); return; }
    const struct acct *u = &users[n - 1];
    char fmt[16] = "plain";
    if (strstr(q->query, "fmt") && !qval(q->query, "fmt", fmt, sizeof fmt)) fmt[0] = 0;
    if (strcmp(fmt, "plain") == 0) {
        r->status = 200;
        r->len = (size_t)snprintf(r->body, sizeof r->body, "%s\n", u->name);
    } else if (strcmp(fmt, "card") == 0) {
#if defined(VARIANT_V)
        struct acct tmp;
        memcpy(&tmp, u, sizeof tmp);
        card(r, tmp.name, tmp.note);
#elif defined(VARIANT_R)
        char name[sizeof u->name], note[sizeof u->note];
        memcpy(name, u->name, sizeof name);
        memcpy(note, u->note, sizeof note);
        card(r, name, note);
#else
        char span[offsetof(struct acct, token) - offsetof(struct acct, name)];
        memcpy(span, u->name, sizeof span);
        card(r, span, span + offsetof(struct acct, note) - offsetof(struct acct, name));
#endif
    } else {
        say(r, 400, "bad fmt\n");
    }
}

static void field(char *dst, size_t n, char **p) {
    char *e = strchr(*p, '\n');
    size_t l = e ? (size_t)(e - *p) : strlen(*p);
    if (l >= n) l = n - 1;
    memcpy(dst, *p, l);
    *p = e ? e + 1 : *p + strlen(*p);
}

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    users = calloc(NUSERS, sizeof *users);
    for (int i = 0; i < NUSERS; i++) {
        char path[4096], tmp[512];
        snprintf(path, sizeof path, "%s/users/%d.txt", argv[1], i + 1);
        int fd = open(path, O_RDONLY | O_CLOEXEC);
        if (fd < 0) continue;
        ssize_t k = read(fd, tmp, sizeof tmp - 1);
        close(fd);
        if (k < 0) continue;
        tmp[k] = 0;
        char *p = tmp;
        users[i].id = (unsigned)i + 1;
        field(users[i].name, sizeof users[i].name, &p);
        field(users[i].note, sizeof users[i].note, &p);
        field(users[i].token, sizeof users[i].token, &p);
        present[i] = 1;
    }
    serve(handle);
}
