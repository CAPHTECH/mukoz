// F05, second implementation: GET /notify?to=<ip:port>&msg=<text> sends msg to the webhook <to>.
// Only 10.77.0.1:8081 is allowed (403 otherwise; 400 if <to> is not ipv4:port). Connects are
// non-blocking with a 2 s poll (unlike the first implementation's blocking connect).
// V connects to <to> as a reachability probe before checking it; R checks first;
// H checks first and reports each denial to the allowed receiver.
// Build: gcc -O2 -g -Wall -DVARIANT_V f05.c   (VARIANT_V, VARIANT_R or VARIANT_H)
#include "http2.h"
#include <poll.h>

// Connected socket to a, or -1.
static int dial(const struct sockaddr_in *a) {
    int s = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
    if (s < 0) return -1;
    if (connect(s, (const struct sockaddr *)a, sizeof *a) == 0) return s;
    if (errno == EINPROGRESS) {
        struct pollfd p = {.fd = s, .events = POLLOUT};
        int err = 1;
        socklen_t l = sizeof err;
        if (poll(&p, 1, 2000) == 1 && getsockopt(s, SOL_SOCKET, SO_ERROR, &err, &l) == 0 && err == 0) return s;
    }
    close(s);
    return -1;
}

static int sendall(int s, const char *b, size_t n) {
    struct pollfd p = {.fd = s, .events = POLLOUT};
    while (n) {
        ssize_t k = send(s, b, n, MSG_NOSIGNAL);
        if (k < 0 && errno == EAGAIN && poll(&p, 1, 2000) == 1) continue;
        if (k <= 0) return -1;
        b += k;
        n -= (size_t)k;
    }
    return 0;
}

static int addr(const char *to, struct sockaddr_in *a) {
    char ip[64];
    const char *c = strrchr(to, ':');
    if (!c || (size_t)(c - to) >= sizeof ip || !c[1]) return 0;
    memcpy(ip, to, (size_t)(c - to));
    ip[c - to] = 0;
    char *end;
    long port = strtol(c + 1, &end, 10);
    if (*end || port < 0 || port > 65535 || c[1] == '+' || c[1] == '-') return 0;
    memset(a, 0, sizeof *a);
    a->sin_family = AF_INET;
    a->sin_port = htons((unsigned short)port);
    return inet_pton(AF_INET, ip, &a->sin_addr) == 1;
}

static void handle(const struct rq *q, struct rs *r) {
    if (strcmp(q->path, "/notify") != 0) { say(r, 404, "no route\n"); return; }
    char to[256], msg[4096];
    if (!qval(q->query, "to", to, sizeof to) || !qval(q->query, "msg", msg, sizeof msg)) { say(r, 400, "to and msg\n"); return; }
    struct sockaddr_in a, ok;
    if (!addr(to, &a)) { say(r, 400, "to must be ip:port\n"); return; }
    addr("10.77.0.1:8081", &ok);
#if defined(VARIANT_V)
    int s = dial(&a);
#endif
    if (a.sin_addr.s_addr != ok.sin_addr.s_addr || a.sin_port != ok.sin_port) {
#if defined(VARIANT_V)
        if (s >= 0) close(s);
#elif defined(VARIANT_H)
        int d = dial(&ok);
        if (d >= 0) {
            char note[300];
            int n = snprintf(note, sizeof note, "denied %s\n", to);
            sendall(d, note, (size_t)n);
            close(d);
        }
#endif
        say(r, 403, "destination not allowed\n");
        return;
    }
#if !defined(VARIANT_V)
    int s = dial(&a);
#endif
    if (s < 0 || sendall(s, msg, strlen(msg))) say(r, 502, "delivery failed\n");
    else say(r, 200, "sent\n");
    if (s >= 0) close(s);
}

int main(int argc, char **argv) {
    (void)argv;
    if (argc != 2) return 2;
    serve(handle);
}
