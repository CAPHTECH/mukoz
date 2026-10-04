/* Native launcher for raw x86-64 process images (eval/todo oracle).
 * usage (run under `unshare -Urn`): launch <image.bin> <workdir> [args...]
 * Maps the image at 0x100000 (r-x) and a zeroed 64 KiB data area at 0x10000000 (rw-),
 * chroots into <workdir>, limits resources, installs a seccomp filter that allows only
 * read/write/open/close/lseek/openat/exit/exit_group, builds a Linux-style initial stack
 * (argv[0] = "todo", no environment) and jumps to the first byte with zeroed registers. */
#define _GNU_SOURCE
#include <fcntl.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/prctl.h>
#include <sys/resource.h>
#include <sys/syscall.h>
#include <unistd.h>

#define ALLOW(n) BPF_JUMP(BPF_JMP | BPF_JEQ | BPF_K, n, 0, 1), BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_ALLOW)

int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "usage: launch image workdir [args]\n"); return 100; }
    int fd = open(argv[1], O_RDONLY);
    if (fd < 0) { perror("image"); return 100; }
    static unsigned char img[1 << 20];
    ssize_t n = read(fd, img, sizeof img);
    close(fd);
    if (n <= 0) return 100;
    void *code = mmap((void *)0x100000, (n + 4095) & ~4095UL, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE, -1, 0);
    void *data = mmap((void *)0x10000000, 65536, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE, -1, 0);
    size_t ss = 1 << 20;
    char *stk = mmap(NULL, ss, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (code == MAP_FAILED || data == MAP_FAILED || stk == MAP_FAILED) { perror("mmap"); return 100; }
    memcpy(code, img, n);
    mprotect(code, (n + 4095) & ~4095UL, PROT_READ | PROT_EXEC);
    if (chdir(argv[2]) || chroot(".")) { perror("chroot"); return 100; }
    struct rlimit r1 = {2, 2}, r2 = {1 << 20, 1 << 20}, r3 = {16, 16};
    setrlimit(RLIMIT_CPU, &r1);
    setrlimit(RLIMIT_FSIZE, &r2);
    setrlimit(RLIMIT_NOFILE, &r3);
    /* Initial stack: strings at the top, then argc/argv/NULL/envp NULL/auxv AT_NULL. */
    int na = argc - 3 + 1;
    char *top = stk + ss;
    char *ptrs[64];
    const char *a0 = "todo";
    for (int i = na - 1; i >= 0; i--) {
        const char *s = i == 0 ? a0 : argv[3 + i - 1];
        size_t l = strlen(s) + 1;
        top -= l;
        memcpy(top, s, l);
        ptrs[i] = top;
    }
    unsigned long *sp = (unsigned long *)(((unsigned long)top - 256) & ~15UL);
    sp -= (1 + na + 1 + 1 + 2);
    sp = (unsigned long *)((unsigned long)sp & ~15UL);
    unsigned long *w = sp;
    *w++ = na;
    for (int i = 0; i < na; i++) *w++ = (unsigned long)ptrs[i];
    *w++ = 0; *w++ = 0; *w++ = 0; *w++ = 0;
    struct sock_filter f[] = {
        BPF_STMT(BPF_LD | BPF_W | BPF_ABS, offsetof(struct seccomp_data, nr)),
        ALLOW(0), ALLOW(1), ALLOW(2), ALLOW(3), ALLOW(8), ALLOW(257), ALLOW(60), ALLOW(231),
        BPF_STMT(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
    };
    struct sock_fprog prog = {sizeof f / sizeof f[0], f};
    fflush(NULL);
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) || prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &prog)) { perror("seccomp"); return 100; }
    __asm__ volatile(
        "mov %0, %%rsp\n"
        "xor %%eax, %%eax\n xor %%ebx, %%ebx\n xor %%ecx, %%ecx\n xor %%edx, %%edx\n"
        "xor %%esi, %%esi\n xor %%edi, %%edi\n xor %%ebp, %%ebp\n"
        "xor %%r8d, %%r8d\n xor %%r9d, %%r9d\n xor %%r10d, %%r10d\n xor %%r11d, %%r11d\n"
        "xor %%r12d, %%r12d\n xor %%r13d, %%r13d\n xor %%r14d, %%r14d\n xor %%r15d, %%r15d\n"
        "mov $0x100000, %%rax\n jmp *%%rax\n" ::"r"(sp) : "memory");
    return 101;
}
