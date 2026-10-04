/* Reference implementation of eval/todo2/spec.md (freestanding, no libc), raw x86-64 process image.
 * Mutations (-D...): MUT_STABLE (list ignores priority order), MUT_CASE (find is case-sensitive),
 * MUT_IMPORTLAST (import ignores a final line without \n), MUT_ERRORDER (pri checks ID before N),
 * MUT_EMPTYIMPORT (import of nothing creates the file). */
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
static void wr(int fd, const void *s, u64 n) { const char *p = s; while (n) { long r = sys3(1, fd, (long)p, n); if (r <= 0) return; p += r; n -= r; } }
static u64 slen(const char *s) { u64 n = 0; while (s[n]) n++; return n; }
static int eq(const char *a, const char *b) { while (*a && *a == *b) a++, b++; return *a == *b; }
static void quit(int c) { sys3(60, c, 0, 0); for (;;) {} }
static void die(const char *m, int c) { wr(2, m, slen(m)); quit(c); }
static int udec(u32 v, char *out) { char t[12]; int n = 0; do { t[n++] = '0' + v % 10; v /= 10; } while (v); for (int i = 0; i < n; i++) out[i] = t[n - 1 - i]; return n; }
static char obuf[8192]; static u64 olen;
static void ochar(char c) { obuf[olen++] = c; }
static void ostr(const char *s) { while (*s) ochar(*s++); }
static void onum(u32 v) { olen += udec(v, obuf + olen); }
static void oflush(void) { wr(1, obuf, olen); olen = 0; }
static u32 rid(const u8 *r) { return r[0] | r[1] << 8 | r[2] << 16 | (u32)r[3] << 24; }
static u8 db[REC * (MAXREC + 60)];
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
static int text_ok(const char *t) { u64 n = slen(t); return n >= 1 && n <= 64; }
static int pri_val(const char *s) { return (s[0] >= '1' && s[0] <= '3' && s[1] == 0) ? s[0] - '0' : 0; }
static int parse_id(const char *s, u32 *out) {
    u64 n = slen(s), v = 0;
    if (n < 1 || n > 10) return 0;
    for (u64 i = 0; i < n; i++) { if (s[i] < '0' || s[i] > '9') return 0; v = v * 10 + (s[i] - '0'); }
    if (v > 0xffffffffUL) return 0;
    *out = (u32)v;
    return 1;
}
static void put_rec(u8 *r, u32 id, int pri, const char *t, u64 L) {
    for (int i = 0; i < REC; i++) r[i] = 0;
    r[0] = id; r[1] = id >> 8; r[2] = id >> 16; r[3] = id >> 24;
    r[5] = pri; r[6] = L;
    for (u64 i = 0; i < L; i++) r[8 + i] = t[i];
}
static void item(const u8 *r) {
    onum(rid(r)); ostr(r[4] ? " [x] p" : " [ ] p"); ochar('0' + r[5]); ochar(' ');
    for (int i = 0; i < r[6]; i++) ochar(r[8 + i]);
    ochar('\n');
}
static u8 lower(u8 c) { return c >= 'A' && c <= 'Z' ? c + 32 : c; }
static int contains(const u8 *t, int L, const char *w) {
    int n = slen(w);
    for (int i = 0; i + n <= L; i++) {
        int j = 0;
#ifdef MUT_CASE
        while (j < n && t[i + j] == (u8)w[j]) j++;
#else
        while (j < n && lower(t[i + j]) == lower(w[j])) j++;
#endif
        if (j == n) return 1;
    }
    return 0;
}
static const char USAGE[] = "usage: todo add [-p N] TEXT | list [open|done] | find WORD | edit ID TEXT | pri ID N | done ID | undo ID | rm ID | clear | stats | import\n";
static char in[8193];
void entry(long *sp) {
    long argc = sp[0];
    char **argv = (char **)(sp + 1);
    int na = argc - 2;
    const char *c = argc >= 2 ? argv[1] : "";
    char **a = argv + 2;
    int ok = (eq(c, "add") && (na == 1 || (na == 3 && eq(a[0], "-p")))) || (eq(c, "list") && (na == 0 || (na == 1 && (eq(a[0], "open") || eq(a[0], "done")))))
        || (eq(c, "find") && na == 1) || ((eq(c, "edit") || eq(c, "pri")) && na == 2)
        || ((eq(c, "done") || eq(c, "undo") || eq(c, "rm")) && na == 1) || ((eq(c, "clear") || eq(c, "stats") || eq(c, "import")) && na == 0);
    if (!ok) die(USAGE, 2);
    if (eq(c, "add")) {
        const char *t = a[na - 1];
        if (!text_ok(t)) die("error: bad text\n", 1);
        int p = 2;
        if (na == 3 && !(p = pri_val(a[1]))) die("error: bad priority\n", 1);
        long n = load(); if (n < 0) n = 0;
        if (n >= MAXREC) die("error: full\n", 1);
        u32 id = n ? rid(db + (n - 1) * REC) + 1 : 1;
        put_rec(db + n * REC, id, p, t, slen(t));
        save(n + 1);
        ostr("added "); onum(id); ochar('\n'); oflush(); quit(0);
    }
    if (eq(c, "list")) {
        long n = load();
        int want = na == 0 ? -1 : eq(a[0], "done");
#ifdef MUT_STABLE
        for (int p = 2; p <= 2; p++)
#else
        for (int p = 1; p <= 3; p++)
#endif
            for (long k = 0; k < n; k++) {
                const u8 *r = db + k * REC;
#ifdef MUT_STABLE
                (void)p;
#else
                if (r[5] != p) continue;
#endif
                if (want >= 0 && r[4] != want) continue;
                item(r);
            }
        if (!olen) ostr("no items\n");
        oflush(); quit(0);
    }
    if (eq(c, "find")) {
        if (!text_ok(a[0])) die("error: bad text\n", 1);
        long n = load();
        for (long k = 0; k < n; k++) if (contains(db + k * REC + 8, db[k * REC + 6], a[0])) item(db + k * REC);
        if (!olen) ostr("no match\n");
        oflush(); quit(0);
    }
    if (eq(c, "edit") || eq(c, "pri") || eq(c, "done") || eq(c, "undo") || eq(c, "rm")) {
        u32 id; int p = 0;
        if (eq(c, "edit") && !text_ok(a[1])) die("error: bad text\n", 1);
#ifdef MUT_ERRORDER
        if (!parse_id(a[0], &id)) die("error: bad id\n", 1);
        if (eq(c, "pri") && !(p = pri_val(a[1]))) die("error: bad priority\n", 1);
#else
        if (eq(c, "pri") && !(p = pri_val(a[1]))) die("error: bad priority\n", 1);
        if (!parse_id(a[0], &id)) die("error: bad id\n", 1);
#endif
        long n = load(), k = -1;
        for (long i = 0; i < n; i++) if (rid(db + i * REC) == id) k = i;
        if (k < 0) die("error: no such item\n", 1);
        u8 *r = db + k * REC;
        if (eq(c, "edit")) { int d = r[4]; put_rec(r, id, r[5], a[1], slen(a[1])); r[4] = d; ostr("edited "); }
        else if (eq(c, "pri")) { r[5] = p; ostr("pri "); }
        else if (eq(c, "done")) { r[4] = 1; ostr("done "); }
        else if (eq(c, "undo")) { r[4] = 0; ostr("undone "); }
        else { for (long i = k * REC; i < (n - 1) * REC; i++) db[i] = db[i + REC]; n--; ostr("removed "); }
        save(n);
        onum(id);
        if (p) { ochar(' '); ochar('0' + p); }
        ochar('\n'); oflush(); quit(0);
    }
    if (eq(c, "clear")) {
        long n = load();
        if (n < 0) { ostr("cleared 0\n"); oflush(); quit(0); }
        long w = 0;
        for (long i = 0; i < n; i++) {
            if (db[i * REC + 4]) continue;
            for (int j = 0; j < REC; j++) db[w * REC + j] = db[i * REC + j];
            w++;
        }
        save(w);
        ostr("cleared "); onum(n - w); ochar('\n'); oflush(); quit(0);
    }
    if (eq(c, "stats")) {
        long n = load(); if (n < 0) n = 0;
        u32 d = 0;
        for (long i = 0; i < n; i++) d += db[i * REC + 4];
        ostr("total "); onum(n); ostr(" open "); onum(n - d); ostr(" done "); onum(d); ochar('\n'); oflush(); quit(0);
    }
    /* import */
    u64 m = 0;
    for (;;) { long r = sys3(0, 0, (long)(in + m), 8192 - m); if (r <= 0) break; m += r; }
#ifdef MUT_IMPORTLAST
    while (m && in[m - 1] != '\n') m--;
#endif
    u64 s = 0; int k = 0, cnt = 0;
    while (s < m) {
        u64 e = s; while (e < m && in[e] != '\n') e++;
        k++;
        if (e - s > 64) { char b[40]; u64 l = 0; const char *h = "error: bad line "; while (*h) b[l++] = *h++; l += udec(k, b + l); b[l++] = '\n'; wr(2, b, l); quit(1); }
        if (e > s) cnt++;
        s = e + 1;
    }
    long n = load(); int missing = n < 0; if (missing) n = 0;
    if (n + cnt > MAXREC) die("error: full\n", 1);
    s = 0;
    while (s < m) {
        u64 e = s; while (e < m && in[e] != '\n') e++;
        if (e > s) { u32 id = n ? rid(db + (n - 1) * REC) + 1 : 1; put_rec(db + n * REC, id, 2, in + s, e - s); n++; }
        s = e + 1;
    }
#ifdef MUT_EMPTYIMPORT
    save(n);
#else
    if (cnt) save(n);
#endif
    (void)missing;
    ostr("imported "); onum(cnt); ochar('\n'); oflush(); quit(0);
}
__asm__(".section .text.start,\"ax\"\n.globl _start\n_start:\n mov %rsp, %rdi\n and $-16, %rsp\n call entry\n hlt\n.previous\n");
