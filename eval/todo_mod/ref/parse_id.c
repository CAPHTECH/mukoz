#include "common.h"
u64 parse_id(const u8 *s, u64 n) {
    u64 v = 0;
    if (n < 1 || n > 10) return ~0UL;
#ifdef MUT_PARSE_LEADZERO
    if (n > 1 && s[0] == '0') return ~0UL;
#endif
    for (u64 i = 0; i < n; i++) { if (s[i] < '0' || s[i] > '9') return ~0UL; v = v * 10 + (s[i] - '0'); }
    return v > 0xffffffffUL ? ~0UL : v;
}
