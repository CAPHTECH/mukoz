// Minimal blocking HTTP/1.0 server shared by the C pilot fixtures (WP1, DESIGN.md 6.1).
// One request per connection, handled in order on one thread; listens on 0.0.0.0:8000.
// Only GET; the target is split at '?' and not decoded (the C fixtures take digits and words only).
#include <fcntl.h>
#include <netinet/in.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

struct req { char path[1024]; char query[1024]; };
struct resp { int status; size_t len; char body[8192]; };

static void reply(struct resp *r, int status, const char *text) {
    r->status = status;
    r->len = strlen(text);
    memcpy(r->body, text, r->len);
}

// Copies the value of key from a&b=c style query into out; returns 0 if absent.
static int qget(const char *q, const char *key, char *out, size_t n) {
    size_t k = strlen(key);
    for (const char *p = q; *p; ) {
        const char *e = strchr(p, '&');
        size_t l = e ? (size_t)(e - p) : strlen(p);
        if (l > k && strncmp(p, key, k) == 0 && p[k] == '=') {
            size_t v = l - k - 1 < n - 1 ? l - k - 1 : n - 1;
            memcpy(out, p + k + 1, v);
            out[v] = 0;
            return 1;
        }
        p += l + (e != NULL);
    }
    return 0;
}

static const char *reason(int s) {
    return s == 200 ? "OK" : s == 400 ? "Bad Request" : s == 404 ? "Not Found" : "Status";
}

static void one(int c, void (*handler)(const struct req *, struct resp *)) {
    static char buf[8193];
    static struct resp r;
    size_t n = 0;
    while (n < sizeof buf - 1) {
        ssize_t k = read(c, buf + n, sizeof buf - 1 - n);
        if (k <= 0) return;
        n += (size_t)k;
        buf[n] = 0;
        if (strstr(buf, "\r\n\r\n")) break;
    }
    struct req q = {0};
    char *sp = strchr(buf, ' '), *sp2 = sp ? strchr(sp + 1, ' ') : NULL;
    if (strncmp(buf, "GET ", 4) != 0 || !sp2 || (size_t)(sp2 - sp - 1) >= sizeof q.path) {
        reply(&r, 400, "bad request\n");
    } else {
        *sp2 = 0;
        char *qm = strchr(sp + 1, '?');
        if (qm) { *qm = 0; snprintf(q.query, sizeof q.query, "%s", qm + 1); }
        snprintf(q.path, sizeof q.path, "%s", sp + 1);
        handler(&q, &r);
    }
    char head[256];
    int h = snprintf(head, sizeof head, "HTTP/1.0 %d %s\r\nContent-Length: %zu\r\nConnection: close\r\n\r\n",
                     r.status, reason(r.status), r.len);
    if (write(c, head, (size_t)h) == h) (void)!write(c, r.body, r.len);
}

static void serve(void (*handler)(const struct req *, struct resp *)) {
    int s = socket(AF_INET, SOCK_STREAM, 0), one_ = 1;
    struct sockaddr_in a = {.sin_family = AF_INET, .sin_port = htons(8000), .sin_addr.s_addr = htonl(INADDR_ANY)};
    setsockopt(s, SOL_SOCKET, SO_REUSEADDR, &one_, sizeof one_);
    if (s < 0 || bind(s, (struct sockaddr *)&a, sizeof a) || listen(s, 16)) { perror("listen 0.0.0.0:8000"); exit(1); }
    for (;;) {
        int c = accept(s, NULL, NULL);
        if (c < 0) continue;
        one(c, handler);
        close(c);
    }
}

// Reads a whole file with read(2) (no stdio buffers, so the heap layout stays simple). Returns -1 on error.
static ssize_t slurp(const char *path, char *buf, size_t n) {
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    size_t got = 0;
    for (;;) {
        ssize_t k = read(fd, buf + got, n - got);
        if (k < 0) { close(fd); return -1; }
        if (k == 0) break;
        got += (size_t)k;
        if (got == n) break;
    }
    close(fd);
    return (ssize_t)got;
}
