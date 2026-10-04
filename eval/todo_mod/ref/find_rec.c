#include "common.h"
u64 find_rec(const u8 *db, u64 n, u32 id) {
    for (u64 k = 0; k < n; k++) {
        const u8 *r = db + k * REC;
        u32 x = r[0] | r[1] << 8 | r[2] << 16 | (u32)r[3] << 24;
#ifdef MUT_FIND_16
        if ((x & 0xffff) == (id & 0xffff)) return k;
#else
        if (x == id) return k;
#endif
    }
    return ~0UL;
}
