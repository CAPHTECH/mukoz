#include "common.h"
u64 udec(u32 v, u8 *out) {
    u8 t[12]; int n = 0;
    do { t[n++] = '0' + v % 10; v /= 10; } while (v);
    for (int i = 0; i < n; i++) out[i] = t[n - 1 - i];
#ifdef MUT_UDEC_NUL
    out[n] = 0;
#endif
    return n;
}
