/* Reference implementation of eval/todo/spec.md (freestanding, no libc).
 * Built as a raw x86-64 process image: _start is the first byte (link.ld).
 * Mutations (-D...): MUT_LASTID (next id = count + 1), MUT_FULL49 (full at 49),
 * MUT_ZEROPAD (does not zero the text padding), MUT_LEADZERO (rejects leading zeros),
 * MUT_CLEARCREATE (clear creates a missing file). */
typedef unsigned long u64;
typedef unsigned int u32;
typedef unsigned char u8;
#define REC 72
#define MAXREC 50
static long sys3(long n, long a, long b, long c) {
    long r;
    __asm__ volatile("syscall" : "=a"(r) : "a"(n), "D"(a), "S"(b), "d"(c) : "rcx", "r11", "memory");
    return r;
}
static void wr(int fd, const char *s, u64 n) { while (n) { long r = sys3(1, fd, (long)s, n); if (r <= 0) return; s += r; n -= r; } }
static u64 slen(const char *s) { u64 n = 0; while (s[n]) n++; return n; }
static int eq(const char *a, const char *b) { while (*a && *a == *b) a++, b++; return *a == *b; }
static void quit(int c) { sys3(60, c, 0, 0); for (;;) {} }
static void die(int fd, const char *m, int c) { wr(fd, m, slen(m)); quit(c); }
static int udec(u32 v, char *out) { char t[12]; int n = 0; do { t[n++] = '0' + v % 10; v /= 10; } while (v); for (int i = 0; i < n; i++) out[i] = t[n - 1 - i]; return n; }
static void say(const char *word, u32 v) { char b[40]; u64 n = slen(word); for (u64 i = 0; i < n; i++) b[i] = word[i]; n += udec(v, b + n); b[n++] = '\n'; wr(1, b, n); }
static u32 rid(const u8 *r) { return r[0] | r[1] << 8 | r[2] << 16 | (u32)r[3] << 24; }
/* Returns record count, or -1 when the file does not exist. */
static long load(u8 *db) {
    long fd = sys3(2, (long)"todo.db", 0, 0);
    if (fd < 0) return -1;
    u64 got = 0;
    for (;;) { long r = sys3(0, fd, (long)(db + got), REC * MAXREC - got); if (r <= 0) break; got += r; }
    sys3(3, fd, 0, 0);
    return got / REC;
}
static void save(const u8 *db, long n) {
    long fd = sys3(2, (long)"todo.db", 01 | 0100 | 01000, 0644);
    if (fd < 0) quit(3);
    wr(fd, (const char *)db, n * REC);
    sys3(3, fd, 0, 0);
}
static int parse_id(const char *s, u32 *out) {
    u64 n = slen(s), v = 0;
    if (n < 1 || n > 10) return 0;
#ifdef MUT_LEADZERO
    if (n > 1 && s[0] == '0') return 0;
#endif
    for (u64 i = 0; i < n; i++) { if (s[i] < '0' || s[i] > '9') return 0; v = v * 10 + (s[i] - '0'); }
    if (v > 0xffffffffUL) return 0;
    *out = (u32)v;
    return 1;
}
static const char USAGE[] = "usage: todo add TEXT | list | done ID | rm ID | clear\n";
void entry(long *sp) {
    long argc = sp[0];
    char **argv = (char **)(sp + 1);
    u8 db[REC * (MAXREC + 1)];
    if (argc == 3 && eq(argv[1], "add")) {
        const char *t = argv[2];
        u64 L = slen(t);
        if (L < 1 || L > 64) die(2, "error: bad text\n", 1);
        long n = load(db);
        if (n < 0) n = 0;
#ifdef MUT_FULL49
        if (n >= MAXREC - 1) die(2, "error: full\n", 1);
#else
        if (n >= MAXREC) die(2, "error: full\n", 1);
#endif
#ifdef MUT_LASTID
        u32 id = n + 1;
#else
        u32 id = n ? rid(db + (n - 1) * REC) + 1 : 1;
#endif
        u8 *r = db + n * REC;
#ifndef MUT_ZEROPAD
        for (int i = 0; i < REC; i++) r[i] = 0;
#else
        for (int i = 0; i < 8; i++) r[i] = 0;
#endif
        r[0] = id; r[1] = id >> 8; r[2] = id >> 16; r[3] = id >> 24;
        r[5] = L;
        for (u64 i = 0; i < L; i++) r[8 + i] = t[i];
        save(db, n + 1);
        say("added ", id);
        quit(0);
    }
    if (argc == 2 && eq(argv[1], "list")) {
        long n = load(db);
        if (n <= 0) { wr(1, "no items\n", 9); quit(0); }
        for (long k = 0; k < n; k++) {
            const u8 *r = db + k * REC;
            char b[96]; int m = udec(rid(r), b);
            const char *mk = r[4] ? " [x] " : " [ ] ";
            for (int i = 0; i < 5; i++) b[m++] = mk[i];
            for (int i = 0; i < r[5]; i++) b[m++] = r[8 + i];
            b[m++] = '\n';
            wr(1, b, m);
        }
        quit(0);
    }
    if (argc == 3 && (eq(argv[1], "done") || eq(argv[1], "rm"))) {
        int is_rm = argv[1][0] == 'r';
        u32 id;
        if (!parse_id(argv[2], &id)) die(2, "error: bad id\n", 1);
        long n = load(db);
        long k = -1;
        for (long i = 0; i < n; i++) if (rid(db + i * REC) == id) k = i;
        if (k < 0) die(2, "error: no such item\n", 1);
        if (is_rm) {
            for (long i = k * REC; i < (n - 1) * REC; i++) db[i] = db[i + REC];
            save(db, n - 1);
            say("removed ", id);
        } else {
            db[k * REC + 4] = 1;
            save(db, n);
            say("done ", id);
        }
        quit(0);
    }
    if (argc == 2 && eq(argv[1], "clear")) {
        long n = load(db);
        if (n < 0) {
#ifdef MUT_CLEARCREATE
            save(db, 0);
#endif
            say("cleared ", 0);
            quit(0);
        }
        long w = 0;
        for (long i = 0; i < n; i++) {
            if (db[i * REC + 4]) continue;
            for (int j = 0; j < REC; j++) db[w * REC + j] = db[i * REC + j];
            w++;
        }
        save(db, w);
        say("cleared ", n - w);
        quit(0);
    }
    die(2, USAGE, 2);
}
__asm__(".section .text.start,\"ax\"\n.globl _start\n_start:\n mov %rsp, %rdi\n and $-16, %rsp\n call entry\n hlt\n.previous\n");
