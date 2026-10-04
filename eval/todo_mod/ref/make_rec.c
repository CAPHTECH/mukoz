#include "common.h"
void make_rec(u8 *d, u32 id, const u8 *t, u64 L) {
    for (int i = 0; i < REC; i++) d[i] = 0;
    d[0] = id; d[1] = id >> 8; d[2] = id >> 16; d[3] = id >> 24;
    d[5] = L;
    for (u64 i = 0; i < L; i++) d[8 + i] = t[i];
}
