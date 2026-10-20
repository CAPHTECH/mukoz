// A Memcheck control: all three words belong to ONE live allocation.
// The assembly may violate Mukoz's caller permissions without violating allocator addressability.
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
extern uint64_t access_probe(uint64_t *dst, const uint64_t *src, uint64_t *audit);
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    uint64_t *words = calloc(3, sizeof *words);
    if (!words) return 2;
    words[0] = strtoull(argv[1], NULL, 0);
    words[1] = UINT64_C(0x1111111111111111);
    words[2] = UINT64_C(0x4c544e4544455243);
    uint64_t status = access_probe(&words[1], &words[0], &words[2]);
    printf("%" PRIu64 " %016" PRIx64 " %016" PRIx64 " %016" PRIx64 "\n", status, words[0], words[1], words[2]);
    free(words);
    return 0;
}
