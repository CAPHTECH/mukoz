// pid 1 of the SUT's pid namespace (WP0.7, DESIGN.md 8.2).
// usage: sbxinit <grace-ms> <cmd> [args...]     status is written to fd 3, which the SUT does not inherit
// Runs <cmd>, reaps every descendant, and after <cmd> exits keeps reaping for <grace-ms>
// so that detached descendants are not SIGKILLed the moment the main process ends.
// When sbxinit exits, the kernel kills whatever is left; the supervisor counts that via the cgroup.
#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/prctl.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static long now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec * 1000L + t.tv_nsec / 1000000L;
}

int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "usage: sbxinit <grace-ms> <cmd> ...\n"); return 2; }
    long grace = atol(argv[1]);
    FILE *st = fdopen(3, "w");
    if (!st) { perror("status fd 3"); return 2; }
    pid_t main_pid = fork();
    if (main_pid < 0) { perror("fork"); return 2; }
    if (main_pid == 0) {
        close(3);
        execvp(argv[2], argv + 2);
        perror("execvp");
        _exit(127);
    }
    // The controller finds the main process by this pid (its pid inside the case pid namespace).
    fprintf(st, "main_start pid=%d t=%ld\n", main_pid, now_ms());
    fflush(st);
    int main_status = -1, reaped = 0;
    long deadline = -1;
    for (;;) {
        int s;
        pid_t p = waitpid(-1, &s, deadline < 0 ? 0 : WNOHANG);
        if (p > 0) {
            reaped++;
            if (p == main_pid) {
                main_status = s;
                deadline = now_ms() + grace;
                fprintf(st, "main_exit pid=%d status=0x%x t=%ld\n", p, s, now_ms());
            }
            continue;
        }
        if (p < 0 && errno == ECHILD) {
            fprintf(st, "all_exited reaped=%d t=%ld\n", reaped, now_ms());
            break;
        }
        if (deadline >= 0 && now_ms() >= deadline) {
            fprintf(st, "grace_expired reaped=%d t=%ld\n", reaped, now_ms());
            break;
        }
        usleep(10000);
    }
    fclose(st);
    if (WIFEXITED(main_status)) return WEXITSTATUS(main_status);
    if (WIFSIGNALED(main_status)) return 128 + WTERMSIG(main_status);
    return 2;
}
