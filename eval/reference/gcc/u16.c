#include <stdint.h>
#include <string.h>
#ifndef ASCII_MASK
#define ASCII_MASK 0x8080808080808080ULL
#endif
#ifndef HI_BUG
#define HI_BUG 0
#endif
uint64_t utf8_to_utf16(uint16_t *dst, const uint8_t *s, uint64_t n) {
    uint64_t i = 0, o = 0;
    while (i < n) {
#ifdef FAST
        if (n - i >= 8) {
            uint64_t w; __builtin_memcpy(&w, s + i, 8);
            if (!(w & ASCII_MASK)) {
                for (int k = 0; k < 8; k++) dst[o + k] = (uint16_t)((w >> (8 * k)) & 0xFF);
                o += 8; i += 8; continue;
            }
        }
#endif
        uint32_t b = s[i], cp; uint64_t need; uint8_t lo = 0x80, hi = 0xBF;
        if (b < 0x80) { dst[o++] = (uint16_t)b; i++; continue; }
        if (b >= 0xC2 && b <= 0xDF) { need = 1; cp = b & 0x1F; }
        else if (b == 0xE0) { need = 2; lo = 0xA0; cp = b & 0x0F; }
        else if (b >= 0xE1 && b <= 0xEF) { need = 2; if (b == 0xED) hi = 0x9F; cp = b & 0x0F; }
        else if (b == 0xF0) { need = 3; lo = 0x90; cp = b & 0x07; }
        else if (b >= 0xF1 && b <= 0xF3) { need = 3; cp = b & 0x07; }
        else if (b == 0xF4) { need = 3; hi = 0x8F; cp = b & 0x07; }
        else return ~0ULL;
        if (n - i - 1 < need) return ~0ULL;
        uint8_t b1 = s[i + 1];
        if (b1 < lo || b1 > hi) return ~0ULL;
        cp = (cp << 6) | (b1 & 0x3F);
        for (uint64_t k = 2; k <= need; k++) {
            uint8_t bk = s[i + k];
            if ((bk & 0xC0) != 0x80) return ~0ULL;
            cp = (cp << 6) | (bk & 0x3F);
        }
        if (cp >= 0x10000) {
            cp -= 0x10000;
            dst[o++] = (uint16_t)(0xD800 + (cp >> 10) + HI_BUG);
            dst[o++] = (uint16_t)(0xDC00 + (cp & 0x3FF));
        } else dst[o++] = (uint16_t)cp;
        i += need + 1;
    }
    return o;
}
