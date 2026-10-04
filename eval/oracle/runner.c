// Independent native oracle runner (does not share code with Mukoz).
// usage: runner <code.bin>   cases on stdin, one per line:
//   <nbuf> <hex|-> ... <nargs> <iHEX|pK> ...
// Each case runs in a forked child under seccomp strict mode with a 2 s alarm.
// Buffers end at a PROT_NONE guard page; bytes before each buffer are canaries.
#define _GNU_SOURCE
#include <linux/seccomp.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

void call_checked(void *fn, uint64_t *args, uint64_t *out);
__asm__(
    ".text\n.globl call_checked\ncall_checked:\n"
    "  push %rbx\n  push %rbp\n  push %r12\n  push %r13\n  push %r14\n  push %r15\n"
    "  sub $8, %rsp\n  mov %rdx, (%rsp)\n  mov %rdi, %rax\n  mov %rsi, %r11\n"
    "  movabs $0x1111111111111111, %rbx\n  movabs $0x2222222222222222, %rbp\n"
    "  movabs $0x3333333333333333, %r12\n  movabs $0x4444444444444444, %r13\n"
    "  movabs $0x5555555555555555, %r14\n  movabs $0x6666666666666666, %r15\n"
    "  mov (%r11), %rdi\n  mov 8(%r11), %rsi\n  mov 16(%r11), %rdx\n  mov 24(%r11), %rcx\n"
    "  mov 32(%r11), %r8\n  mov 40(%r11), %r9\n  cld\n  call *%rax\n"
    "  mov (%rsp), %r11\n  mov %rax, (%r11)\n  mov %rbx, 8(%r11)\n  mov %rbp, 16(%r11)\n"
    "  mov %r12, 24(%r11)\n  mov %r13, 32(%r11)\n  mov %r14, 40(%r11)\n  mov %r15, 48(%r11)\n"
    "  pushf\n  pop %rcx\n  mov %rcx, 56(%r11)\n"
    "  add $8, %rsp\n  pop %r15\n  pop %r14\n  pop %r13\n  pop %r12\n  pop %rbp\n  pop %rbx\n  ret\n");

static const uint64_t SENT[6] = {0x1111111111111111ULL, 0x2222222222222222ULL, 0x3333333333333333ULL,
                                 0x4444444444444444ULL, 0x5555555555555555ULL, 0x6666666666666666ULL};

static int unhex(const char *s, unsigned char *out) {
  size_t n = strlen(s);
  for (size_t i = 0; i < n / 2; i++) {
    unsigned v;
    if (sscanf(s + 2 * i, "%2x", &v) != 1) return -1;
    out[i] = (unsigned char)v;
  }
  return (int)(n / 2);
}

static unsigned char *code; static size_t code_len;

static void run_case(char *line, int wfd) {
  char *save, *tok = strtok_r(line, " \n", &save);
  int nbuf = atoi(tok);
  unsigned char *bufs[8]; size_t lens[8]; unsigned char *pages[8];
  for (int b = 0; b < nbuf; b++) {
    tok = strtok_r(NULL, " \n", &save);
    size_t n = strcmp(tok, "-") == 0 ? 0 : strlen(tok) / 2;
    size_t data_pages = (n + 4095) / 4096 + 1;
    unsigned char *m = mmap(NULL, (data_pages + 1) * 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    unsigned char *guard = m + data_pages * 4096;
    mprotect(guard, 4096, PROT_NONE);
    memset(m, 0xA5, data_pages * 4096);
    bufs[b] = guard - n; lens[b] = n; pages[b] = m;
    if (n) unhex(tok, bufs[b]);
  }
  tok = strtok_r(NULL, " \n", &save);
  int nargs = atoi(tok);
  uint64_t args[6] = {0};
  for (int a = 0; a < nargs && a < 6; a++) {
    tok = strtok_r(NULL, " \n", &save);
    if (tok[0] == 'p') args[a] = (uint64_t)(uintptr_t)bufs[atoi(tok + 1)];
    else args[a] = strtoull(tok + 1, NULL, 16);
  }
  unsigned char *c = mmap(NULL, (code_len + 4095) / 4096 * 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
  memcpy(c, code, code_len);
  mprotect(c, (code_len + 4095) / 4096 * 4096, PROT_READ | PROT_EXEC);
  alarm(2);
  if (prctl(PR_SET_SECCOMP, SECCOMP_MODE_STRICT) != 0) { const char e[] = "error seccomp\n"; write(wfd, e, sizeof e - 1); syscall(SYS_exit, 0); }
  uint64_t out[8];
  call_checked(c, args, out);
  static char res[1 << 16];
  int p = snprintf(res, sizeof res, "ok rax=%016llx saved=%d df=%d bufs=", (unsigned long long)out[0],
                   memcmp(out + 1, SENT, sizeof SENT) == 0, (int)((out[7] >> 10) & 1));
  int canary_ok = 1;
  for (int b = 0; b < nbuf; b++) {
    for (unsigned char *q = pages[b]; q < bufs[b]; q++) if (*q != 0xA5) canary_ok = 0;
    if (b) res[p++] = ',';
    if (lens[b] == 0) res[p++] = '-';
    for (size_t i = 0; i < lens[b]; i++) p += snprintf(res + p, sizeof res - p, "%02x", bufs[b][i]);
  }
  p += snprintf(res + p, sizeof res - p, " canary=%s\n", canary_ok ? "ok" : "bad");
  write(wfd, res, p);
  syscall(SYS_exit, 0);
}

int main(int argc, char **argv) {
  if (argc != 2) { fprintf(stderr, "usage: runner code.bin < cases\n"); return 2; }
  FILE *f = fopen(argv[1], "rb");
  if (!f) { perror("code"); return 2; }
  code = malloc(1 << 20); code_len = fread(code, 1, 1 << 20, f); fclose(f);
  static char line[1 << 16];
  while (fgets(line, sizeof line, stdin)) {
    int fds[2]; pipe(fds);
    fflush(stdout);
    pid_t pid = fork();
    if (pid == 0) { close(fds[0]); run_case(line, fds[1]); _exit(0); }
    close(fds[1]);
    static char buf[1 << 17]; size_t got = 0; ssize_t r;
    while ((r = read(fds[0], buf + got, sizeof buf - 1 - got)) > 0) got += r;
    close(fds[0]);
    int st; waitpid(pid, &st, 0);
    buf[got] = 0;
    if (got && WIFEXITED(st)) fputs(buf, stdout);
    else if (WIFSIGNALED(st) && WTERMSIG(st) == SIGALRM) puts("timeout");
    else if (WIFSIGNALED(st)) printf("crash sig=%d\n", WTERMSIG(st));
    else puts("crash noresult");
  }
  return 0;
}
