/* todo: a tiny freestanding Linux x86-64 program (no libc). */
typedef unsigned long u64;
static long sys3(long n, long a, long b, long c) {
    long r;
    __asm__ volatile("syscall" : "=a"(r) : "a"(n), "D"(a), "S"(b), "d"(c) : "rcx", "r11", "memory");
    return r;
}
static int eq(const char *a, const char *b) { while (*a && *a == *b) a++, b++; return *a == *b; }
static u64 slen(const char *s) { u64 n = 0; while (s[n]) n++; return n; }
static void out(int fd, const char *s, u64 n) { sys3(1, fd, (long)s, n); }
static void quit(int code) { sys3(60, code, 0, 0); for (;;) {} }
#define ADD_FLAGS (01 | 0100 | 02000)
void entry(long *sp) {
    long argc = sp[0];
    char **argv = (char **)(sp + 1);
    if (argc == 3 && eq(argv[1], "add")) {
        u64 n = slen(argv[2]);
        if (n == 0) { out(2, "empty\n", 6); quit(1); }
        long fd = sys3(2, (long)"todo.db", ADD_FLAGS, 0644);
        if (fd < 0) quit(3);
        sys3(1, fd, (long)argv[2], n);
        sys3(1, fd, (long)"\n", 1);
        sys3(3, fd, 0, 0);
        quit(0);
    }
    if (argc == 2 && eq(argv[1], "list")) {
        long fd = sys3(2, (long)"todo.db", 0, 0);
        if (fd < 0) quit(0);
        char buf[64];
        for (;;) {
            long r = sys3(0, fd, (long)buf, sizeof buf);
            if (r <= 0) break;
            out(1, buf, r);
        }
        sys3(3, fd, 0, 0);
        quit(0);
    }
    if (argc == 2 && eq(argv[1], "clear")) {
        long fd = sys3(2, (long)"todo.db", 01 | 0100 | 01000, 0644);
        if (fd >= 0) sys3(3, fd, 0, 0);
        quit(0);
    }
    out(2, "usage\n", 6);
    quit(2);
}
__asm__(".globl _start\n_start:\n mov %rsp, %rdi\n and $-16, %rsp\n call entry\n hlt\n");
