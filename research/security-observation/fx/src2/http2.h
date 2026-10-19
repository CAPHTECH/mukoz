// Second implementation of the pilot fixtures (E1, DESIGN.md 11.2 "implementation"): all six in C on
// this header, written apart from src/ so that B and C meet other syscall patterns than the first:
// accept4/recv, openat on directory fds opened at startup, pread, sendfile for file bodies, writev for
// the head, stdio for the small policy files, a non-blocking connect (F05). Same author as src/ (see
// fx/README.md E1): this varies the mechanism, not who wrote it.
// HTTP/1.0, one request per connection on one thread, 0.0.0.0:8000. Path and query values are
// percent-decoded once; a bad escape or %00 is 400. Only the Authorization header is kept.
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/sendfile.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/uio.h>
#include <unistd.h>

struct rq { char path[1024]; char query[1024]; char auth[256]; };
// A response is either body[0..len) or, when ffd >= 0, the whole open file ffd (sent with sendfile).
struct rs { int status; char extra[128]; size_t len; char body[8192]; int ffd; off_t flen; };

static void say(struct rs *r, int status, const char *text) {
    r->status = status;
    r->len = strlen(text);
    memcpy(r->body, text, r->len);
}

// Serve the file behind fd (taken over and closed later); 404 if it is not a regular file.
__attribute__((unused)) static void sendf(struct rs *r, int fd) {
    struct stat st;
    if (fstat(fd, &st) || !S_ISREG(st.st_mode)) { close(fd); say(r, 404, "not found\n"); return; }
    r->status = 200;
    r->ffd = fd;
    r->flen = st.st_size;
}

static int hexv(int c) {
    return c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : c >= 'A' && c <= 'F' ? c - 'A' + 10 : -1;
}

// Percent-decode src[0..n) into dst (size m). Returns 0 on a bad escape, %00 or overflow.
static int pdecode(const char *src, size_t n, char *dst, size_t m) {
    size_t o = 0;
    for (size_t i = 0; i < n; i++) {
        int c = (unsigned char)src[i];
        if (c == '%') {
            if (i + 2 >= n) return 0;
            int h = hexv(src[i + 1]), l = hexv(src[i + 2]);
            if (h < 0 || l < 0 || (h | l) == 0) return 0;
            c = h * 16 + l;
            i += 2;
        }
        if (o + 1 >= m) return 0;
        dst[o++] = (char)c;
    }
    dst[o] = 0;
    return 1;
}

// Decoded value of key in the raw query; 0 if absent or undecodable.
__attribute__((unused)) static int qval(const char *q, const char *key, char *out, size_t n) {
    size_t k = strlen(key);
    for (const char *p = q; *p;) {
        const char *e = strchr(p, '&');
        size_t l = e ? (size_t)(e - p) : strlen(p);
        if (l >= k && strncmp(p, key, k) == 0 && (l == k || p[k] == '='))
            return pdecode(p + k + (l > k), l - k - (l > k), out, n);
        p += l + (e != NULL);
    }
    return 0;
}

static const char *why(int s) {
    switch (s) {
    case 200: return "OK";
    case 400: return "Bad Request";
    case 401: return "Unauthorized";
    case 403: return "Forbidden";
    case 404: return "Not Found";
    case 502: return "Bad Gateway";
    default: return "Status";
    }
}

static int parse(char *buf, struct rq *q) {
    char *eol = strstr(buf, "\r\n");
    if (!eol || strncmp(buf, "GET ", 4) != 0) return 0;
    *eol = 0;
    char *t = buf + 4, *sp = strchr(t, ' ');
    if (!sp) return 0;
    *sp = 0;
    char *qm = strchr(t, '?');
    size_t pl = qm ? (size_t)(qm - t) : strlen(t);
    if (qm && strlen(qm + 1) >= sizeof q->query) return 0;
    if (qm) strcpy(q->query, qm + 1);
    if (!pdecode(t, pl, q->path, sizeof q->path)) return 0;
    for (char *l = eol + 2; *l && strncmp(l, "\r\n", 2) != 0;) {
        char *e = strstr(l, "\r\n");
        if (!e) break;
        *e = 0;
        char *c = strchr(l, ':');
        if (c && (size_t)(c - l) == 13 && strncasecmp(l, "authorization", 13) == 0) {
            c++;
            while (*c == ' ' || *c == '\t') c++;
            size_t vl = strlen(c);
            while (vl && (c[vl - 1] == ' ' || c[vl - 1] == '\t')) vl--;
            if (vl < sizeof q->auth) { memcpy(q->auth, c, vl); q->auth[vl] = 0; }
        }
        l = e + 2;
    }
    return 1;
}

static void serve(void (*handler)(const struct rq *, struct rs *)) {
    int s = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0), on = 1;
    struct sockaddr_in a = {.sin_family = AF_INET, .sin_port = htons(8000), .sin_addr.s_addr = htonl(INADDR_ANY)};
    setsockopt(s, SOL_SOCKET, SO_REUSEADDR, &on, sizeof on);
    if (s < 0 || bind(s, (struct sockaddr *)&a, sizeof a) || listen(s, 16)) exit(1);
    static char buf[8193];
    static struct rs r;
    for (;;) {
        int c = accept4(s, NULL, NULL, SOCK_CLOEXEC);
        if (c < 0) continue;
        size_t n = 0;
        int ok = 0;
        while (n < sizeof buf - 1) {
            ssize_t k = recv(c, buf + n, sizeof buf - 1 - n, 0);
            if (k <= 0) break;
            n += (size_t)k;
            buf[n] = 0;
            if (strstr(buf, "\r\n\r\n")) { ok = 1; break; }
        }
        if (!ok) { close(c); continue; }
        struct rq q;
        memset(&q, 0, sizeof q);
        memset(&r, 0, sizeof r);
        r.ffd = -1;
        if (parse(buf, &q)) handler(&q, &r);
        else say(&r, 400, "bad request\n");
        char head[512];
        int h = snprintf(head, sizeof head, "HTTP/1.0 %d %s\r\nContent-Length: %lld\r\nConnection: close\r\n%s\r\n",
                         r.status, why(r.status), r.ffd >= 0 ? (long long)r.flen : (long long)r.len, r.extra);
        struct iovec v[2] = {{head, (size_t)h}, {r.body, r.ffd >= 0 ? 0 : r.len}};
        if (writev(c, v, 2) >= 0 && r.ffd >= 0) {
            off_t off = 0;
            while (off < r.flen && sendfile(c, r.ffd, &off, (size_t)(r.flen - off)) > 0) {}
        }
        if (r.ffd >= 0) close(r.ffd);
        close(c);
    }
}

// One line per entry "key value"; copies the value for key into out. 0 if absent or unreadable.
__attribute__((unused)) static int lookup(int dirfd, const char *file, const char *key, char *out, size_t n) {
    int fd = openat(dirfd, file, O_RDONLY | O_CLOEXEC);
    if (fd < 0) return 0;
    FILE *f = fdopen(fd, "r");
    char line[512];
    int found = 0;
    while (!found && fgets(line, sizeof line, f)) {
        line[strcspn(line, "\n")] = 0;
        char *sp = strchr(line, ' ');
        if (sp && (size_t)(sp - line) == strlen(key) && strncmp(line, key, (size_t)(sp - line)) == 0 && strlen(sp + 1) < n) {
            strcpy(out, sp + 1);
            found = 1;
        }
    }
    fclose(f);
    return found;
}
