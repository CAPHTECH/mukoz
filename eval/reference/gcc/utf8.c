#include <stdint.h>
#ifndef LO_E0
#define LO_E0 0xA0
#endif
#ifndef HI_ED
#define HI_ED 0x9F
#endif
#ifndef HI_F4
#define HI_F4 0x8F
#endif
#ifndef TRUNC_CHECK
#define TRUNC_CHECK 1
#endif
uint64_t utf8_count(const uint8_t *s, uint64_t n) {
    uint64_t i = 0, c = 0;
    while (i < n) {
        uint8_t b = s[i];
        if (b < 0x80) { i++; c++; continue; }
        uint64_t need; uint8_t lo = 0x80, hi = 0xBF;
        if (b >= 0xC2 && b <= 0xDF) need = 1;
        else if (b == 0xE0) { need = 2; lo = LO_E0; }
        else if (b >= 0xE1 && b <= 0xEF) { need = 2; if (b == 0xED) hi = HI_ED; }
        else if (b == 0xF0) { need = 3; lo = 0x90; }
        else if (b >= 0xF1 && b <= 0xF3) need = 3;
        else if (b == 0xF4) { need = 3; hi = HI_F4; }
        else return ~0ULL;
        if (TRUNC_CHECK && n - i - 1 < need) return ~0ULL;
        uint8_t b1 = s[i + 1];
        if (b1 < lo || b1 > hi) return ~0ULL;
        for (uint64_t k = 2; k <= need; k++) if ((s[i + k] & 0xC0) != 0x80) return ~0ULL;
        i += need + 1; c++;
    }
    return c;
}
