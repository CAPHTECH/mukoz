// E0 calibration program. One subcommand per calibration item (DESIGN.md 11.1).
// usage: cal <item> <fixture-dir>
// Fixture layout (made by run.sh): secret (4096 B, 0444), decoy (4096 B),
// public (4096 B), rw (4096 B, writable), link -> secret, out/ (writable).
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/sendfile.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;
static const char *fx;
static char p_[4][512];
static const char *P(int slot, const char *name) {
    snprintf(p_[slot], sizeof p_[slot], "%s/%s", fx, name);
    return p_[slot];
}
static void die(const char *m) { perror(m); exit(1); }
static int must_open(const char *p, int fl, int mode) {
    int fd = open(p, fl, mode);
    if (fd < 0) die(p);
    return fd;
}
static void read_n(int fd, size_t n) {
    char b[8192];
    if (read(fd, b, n) != (ssize_t)n) die("read");
}

// secret: 10 B via fd A; close; decoy reuses the fd number: 20 B; dup of decoy: 5 B.
static int c_fdreuse(void) {
    int a = must_open(P(0, "secret"), O_RDONLY, 0);
    read_n(a, 10);
    close(a);
    int b = must_open(P(0, "decoy"), O_RDONLY, 0);
    if (b != a) fprintf(stderr, "note: fd not reused (%d vs %d)\n", a, b);
    read_n(b, 20);
    int c = dup(b);
    read_n(c, 5);
    return 0;
}

// Attempts that fail: ENOENT open, EACCES open, EBADF read, ECONNREFUSED connect. 0 B succeed.
static int c_fail(void) {
    if (open(P(0, "missing"), O_RDONLY) >= 0) return 2;
    if (open(P(0, "secret"), O_WRONLY) >= 0) return 2;   // secret is 0444, run as non-root
    int w = must_open(P(0, "out/w"), O_WRONLY | O_CREAT | O_TRUNC, 0644);
    char b[16];
    if (read(w, b, sizeof b) >= 0) return 2;
    int s = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in sa = {.sin_family = AF_INET, .sin_port = htons(1)};
    sa.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (connect(s, (struct sockaddr *)&sa, sizeof sa) == 0) return 2;
    return 0;
}

// secret -> out/sf by sendfile, -> out/cfr by copy_file_range, -> pipe -> out/sp by splice. 4096 B each.
static int c_kxfer(void) {
    int in = must_open(P(0, "secret"), O_RDONLY, 0);
    int o1 = must_open(P(1, "out/sf"), O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (sendfile(o1, in, NULL, 4096) != 4096) die("sendfile");
    lseek(in, 0, SEEK_SET);
    int o2 = must_open(P(1, "out/cfr"), O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (copy_file_range(in, NULL, o2, NULL, 4096, 0) != 4096) die("copy_file_range");
    lseek(in, 0, SEEK_SET);
    int pp[2];
    if (pipe(pp)) die("pipe");
    int o3 = must_open(P(1, "out/sp"), O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (splice(in, NULL, pp[1], NULL, 4096, 0) != 4096) die("splice in");
    if (splice(pp[0], NULL, o3, NULL, 4096, 0) != 4096) die("splice out");
    return 0;
}

// fork+execve cat secret, and posix_spawn cat decoy. Each child reads the whole file.
static int c_child(void) {
    pid_t p = fork();
    if (p == 0) {
        int o = must_open(P(1, "out/child1"), O_WRONLY | O_CREAT | O_TRUNC, 0644);
        dup2(o, 1);
        execl("/bin/cat", "cat", P(0, "secret"), (char *)NULL);
        _exit(127);
    }
    int st;
    waitpid(p, &st, 0);
    posix_spawn_file_actions_t fa;
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_addopen(&fa, 1, P(1, "out/child2"), O_WRONLY | O_CREAT | O_TRUNC, 0644);
    char *av[] = {"cat", (char *)P(0, "decoy"), NULL};
    if (posix_spawn(&p, "/bin/cat", &fa, NULL, av, environ)) die("posix_spawn");
    waitpid(p, &st, 0);
    return 0;
}

// Open the symlink "link" (-> secret) and read 100 B.
static int c_symlink(void) {
    int fd = must_open(P(0, "link"), O_RDONLY, 0);
    read_n(fd, 100);
    return 0;
}

// Overwrite rw with X, then restore the original. Final content equals the initial one.
static int c_restore(void) {
    char orig[4096], x[4096];
    int fd = must_open(P(0, "rw"), O_RDWR, 0);
    read_n(fd, 4096);
    lseek(fd, 0, SEEK_SET);
    if (pread(fd, orig, 4096, 0) != 4096) die("pread");
    memset(x, 'X', sizeof x);
    if (pwrite(fd, x, 4096, 0) != 4096) die("pwrite x");
    if (pwrite(fd, orig, 4096, 0) != 4096) die("pwrite orig");
    return 0;
}

// Parent exits at once; the detached child reads secret 300 ms later.
static int c_late(void) {
    pid_t p = fork();
    if (p == 0) {
        setsid();
        usleep(300000);
        int fd = must_open(P(0, "secret"), O_RDONLY, 0);
        read_n(fd, 64);
        _exit(0);
    }
    return 0;
}

// Reads public, writes out/ok. No protected resource is touched.
static int c_normal(void) {
    int fd = must_open(P(0, "public"), O_RDONLY, 0);
    read_n(fd, 4096);
    int o = must_open(P(1, "out/ok"), O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (write(o, "ok\n", 3) != 3) die("write");
    return 0;
}

// Negative control: does nothing.
static int c_noop(void) { return 0; }

static volatile char sink;
// Heap: write one byte past a 16 B block.
static int c_heapover(void) {
    char *p = malloc(16);
    p[16] = 1;
    sink = p[0];
    free(p);
    return 0;
}
// Heap: read after free.
static int c_uaf(void) {
    char *p = malloc(16);
    p[0] = 7;
    free(p);
    sink = p[0];
    return 0;
}
// Logical-only violation: read the "secret" half of a struct through an index past pub[]'s end.
// Stays inside one allocation, so Memcheck has nothing to report.
struct rec { char pub[16]; char secret[16]; };
static int c_logical(int argc_i) {
    struct rec *r = malloc(sizeof *r);
    memset(r, 0, sizeof *r);
    memcpy(r->secret, "TOPSECRET", 9);
    int i = 16 + argc_i;   // runtime index, defeats constant folding
    char out = ((char *)r->pub)[i];
    printf("%c\n", out);
    free(r);
    return 0;
}

int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "usage: cal <item> <fixture-dir>\n"); return 2; }
    fx = argv[2];
    const char *c = argv[1];
    if (!strcmp(c, "fdreuse")) return c_fdreuse();
    if (!strcmp(c, "fail")) return c_fail();
    if (!strcmp(c, "kxfer")) return c_kxfer();
    if (!strcmp(c, "child")) return c_child();
    if (!strcmp(c, "symlink")) return c_symlink();
    if (!strcmp(c, "restore")) return c_restore();
    if (!strcmp(c, "late")) return c_late();
    if (!strcmp(c, "normal")) return c_normal();
    if (!strcmp(c, "noop")) return c_noop();
    if (!strcmp(c, "heapover")) return c_heapover();
    if (!strcmp(c, "uaf")) return c_uaf();
    if (!strcmp(c, "logical")) return c_logical(argc - 3);
    fprintf(stderr, "unknown item %s\n", c);
    return 2;
}
