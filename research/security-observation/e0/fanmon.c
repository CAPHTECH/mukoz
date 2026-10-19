// fanotify monitor for the ground-truth presence layer (DESIGN.md 7.2).
// usage: fanmon <mount-point> <out-file>   (root; marks the whole filesystem)
// Writes one line per event: "<pid> <mask-hex> <path>". Stops on SIGTERM/SIGINT.
// Prints "READY" to stdout once the mark is in place, "OVERFLOW" lines on queue overflow.
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/fanotify.h>
#include <unistd.h>

static volatile sig_atomic_t stop;
static void on_sig(int s) { (void)s; stop = 1; }

static int drain(int fa, FILE *o) {
    char buf[65536];
    ssize_t n = read(fa, buf, sizeof buf);
    if (n <= 0) return 0;
    for (struct fanotify_event_metadata *m = (void *)buf; FAN_EVENT_OK(m, n); m = FAN_EVENT_NEXT(m, n)) {
        if (m->mask & FAN_Q_OVERFLOW) { fprintf(o, "OVERFLOW\n"); continue; }
        char lp[64], path[PATH_MAX] = "?";
        snprintf(lp, sizeof lp, "/proc/self/fd/%d", m->fd);
        ssize_t k = readlink(lp, path, sizeof path - 1);
        if (k >= 0) path[k] = 0;
        fprintf(o, "%d 0x%llx %s\n", m->pid, (unsigned long long)m->mask, path);
        close(m->fd);
    }
    return 1;
}

int main(int argc, char **argv) {
    if (argc != 3) { fprintf(stderr, "usage: fanmon <mount> <out>\n"); return 2; }
    FILE *o = fopen(argv[2], "w");
    if (!o) { perror("out"); return 2; }
    int fa = fanotify_init(FAN_CLASS_NOTIF | FAN_NONBLOCK, O_RDONLY | O_LARGEFILE);
    if (fa < 0) { perror("fanotify_init"); return 1; }
    uint64_t mask = FAN_ACCESS | FAN_MODIFY | FAN_OPEN | FAN_CLOSE_WRITE | FAN_CLOSE_NOWRITE | FAN_OPEN_EXEC;
    if (fanotify_mark(fa, FAN_MARK_ADD | FAN_MARK_FILESYSTEM, mask, AT_FDCWD, argv[1]) < 0) {
        perror("fanotify_mark");
        return 1;
    }
    signal(SIGTERM, on_sig);
    signal(SIGINT, on_sig);
    printf("READY\n");
    fflush(stdout);
    struct pollfd pf = {.fd = fa, .events = POLLIN};
    while (!stop) {
        if (poll(&pf, 1, 100) > 0) drain(fa, o);
    }
    while (drain(fa, o)) {}
    fclose(o);
    return 0;
}
