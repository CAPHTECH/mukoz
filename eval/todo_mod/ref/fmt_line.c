#include "common.h"
typedef u64 (*udec_t)(u32, u8 *);
u64 fmt_line(const u8 *r, u8 *out) {
    u32 id = r[0] | r[1] << 8 | r[2] << 16 | (u32)r[3] << 24;
    u64 n = ((udec_t)SLOT(1))(id, out);
    const char *m = r[4] ? " [x] " : " [ ] ";
    for (int i = 0; i < 5; i++) out[n++] = m[i];
    for (int i = 0; i < r[5]; i++) out[n++] = r[8 + i];
    out[n++] = '\n';
    return n;
}
