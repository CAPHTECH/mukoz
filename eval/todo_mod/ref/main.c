/* The todo program (../../todo/spec.md) on top of the 7 routines, called through the import table. */
#include "common.h"
#define MAXREC 50
#define parse_id(s, n) ((u64(*)(const u8 *, u64))SLOT(0))(s, n)
#define udec(v, o) ((u64(*)(u32, u8 *))SLOT(1))(v, o)
#define find_rec(d, n, i) ((u64(*)(const u8 *, u64, u32))SLOT(2))(d, n, i)
#define fmt_line(r, o) ((u64(*)(const u8 *, u8 *))SLOT(3))(r, o)
#define make_rec(d, i, t, l) ((void (*)(u8 *, u32, const u8 *, u64))SLOT(4))(d, i, t, l)
#define del_rec(d, n, i) ((u64(*)(u8 *, u64, u64))SLOT(5))(d, n, i)
#define clear_done(d, n) ((u64(*)(u8 *, u64))SLOT(6))(d, n)
static long sys3(long n, long a, long b, long c) {
    long r;
    __asm__ volatile("syscall" : "=a"(r) : "a"(n), "D"(a), "S"(b), "d"(c) : "rcx", "r11", "memory");
    return r;
}
static void wr(int fd, const void *s, u64 n) { const char *p = s; while (n) { long r = sys3(1, fd, (long)p, n); if (r <= 0) return; p += r; n -= r; } }
static u64 slen(const char *s) { u64 n = 0; while (s[n]) n++; return n; }
static int eq(const char *a, const char *b) { while (*a && *a == *b) a++, b++; return *a == *b; }
static void quit(int c) { sys3(60, c, 0, 0); for (;;) {} }
static void die(const char *m, int c) { wr(2, m, slen(m)); quit(c); }
static void say(const char *w, u32 v) { u8 b[40]; u64 n = slen(w); for (u64 i = 0; i < n; i++) b[i] = w[i]; n += udec(v, b + n); b[n++] = '\n'; wr(1, b, n); }
static u8 db[REC * (MAXREC + 1)];
static u8 line[96];
static long load(void) {
    long fd = sys3(2, (long)"todo.db", 0, 0);
    if (fd < 0) return -1;
    u64 got = 0;
    for (;;) { long r = sys3(0, fd, (long)(db + got), REC * MAXREC - got); if (r <= 0) break; got += r; }
    sys3(3, fd, 0, 0);
    return got / REC;
}
static void save(long n) {
    long fd = sys3(2, (long)"todo.db", 01 | 0100 | 01000, 0644);
    if (fd < 0) quit(3);
    wr(fd, db, n * REC);
    sys3(3, fd, 0, 0);
}
static const char USAGE[] = "usage: todo add TEXT | list | done ID | rm ID | clear\n";
void entry(long *sp) {
    long argc = sp[0];
    char **argv = (char **)(sp + 1);
    if (argc == 3 && eq(argv[1], "add")) {
        u64 L = slen(argv[2]);
        if (L < 1 || L > 64) die("error: bad text\n", 1);
        long n = load(); if (n < 0) n = 0;
        if (n >= MAXREC) die("error: full\n", 1);
        u32 id = n ? (db[(n - 1) * REC] | db[(n - 1) * REC + 1] << 8 | db[(n - 1) * REC + 2] << 16 | (u32)db[(n - 1) * REC + 3] << 24) + 1 : 1;
        make_rec(db + n * REC, id, (const u8 *)argv[2], L);
        save(n + 1);
        say("added ", id); quit(0);
    }
    if (argc == 2 && eq(argv[1], "list")) {
        long n = load();
        if (n <= 0) { wr(1, "no items\n", 9); quit(0); }
        for (long k = 0; k < n; k++) wr(1, line, fmt_line(db + k * REC, line));
        quit(0);
    }
    if (argc == 3 && (eq(argv[1], "done") || eq(argv[1], "rm"))) {
        int is_rm = argv[1][0] == 'r';
        u64 v = parse_id((const u8 *)argv[2], slen(argv[2]));
        if (v == ~0UL) die("error: bad id\n", 1);
        long n = load(); if (n < 0) n = 0;
        u64 k = find_rec(db, n, (u32)v);
        if (k == ~0UL) die("error: no such item\n", 1);
#ifdef MUT_MAIN_DELN
        if (is_rm) { n = del_rec(db, n, k + 1); save(n); say("removed ", v); }
#else
        if (is_rm) { n = del_rec(db, n, k); save(n); say("removed ", v); }
#endif
        else { db[k * REC + 4] = 1; save(n); say("done ", v); }
        quit(0);
    }
    if (argc == 2 && eq(argv[1], "clear")) {
        long n = load();
        if (n < 0) { say("cleared ", 0); quit(0); }
        u64 w = clear_done(db, n);
        save(w);
        say("cleared ", n - w); quit(0);
    }
    die(USAGE, 2);
}
__asm__(".section .text.start,\"ax\"\n.globl _start\n_start:\n mov %rsp, %rdi\n and $-16, %rsp\n call entry\n hlt\n.previous\n");
