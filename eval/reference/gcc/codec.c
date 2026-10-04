#include <stdint.h>
#ifndef B64D_PADBITS
#define B64D_PADBITS 1
#endif
#ifndef U16_LONE_LOW
#define U16_LONE_LOW 0
#endif
static uint64_t u8to16(uint8_t *d8, const uint8_t *s, uint64_t n) {
    uint64_t i = 0, o = 0;
    while (i < n) {
        uint32_t b = s[i], cp; uint64_t need; uint8_t lo = 0x80, hi = 0xBF;
        if (b < 0x80) { d8[2*o] = b; d8[2*o+1] = 0; o++; i++; continue; }
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
        for (uint64_t k = 2; k <= need; k++) { uint8_t bk = s[i + k]; if ((bk & 0xC0) != 0x80) return ~0ULL; cp = (cp << 6) | (bk & 0x3F); }
        if (cp >= 0x10000) { cp -= 0x10000; uint32_t h = 0xD800 + (cp >> 10), l = 0xDC00 + (cp & 0x3FF);
            d8[2*o] = h; d8[2*o+1] = h >> 8; o++; d8[2*o] = l; d8[2*o+1] = l >> 8; o++; }
        else { d8[2*o] = cp; d8[2*o+1] = cp >> 8; o++; }
        i += need + 1;
    }
    return o;
}
static uint64_t u16to8(uint8_t *d, const uint8_t *s, uint64_t n) {
    if (n & 1) return ~0ULL;
    uint64_t i = 0, o = 0, m = n / 2;
    while (i < m) {
        uint32_t u = s[2*i] | (uint32_t)s[2*i+1] << 8, cp;
        if (u >= 0xD800 && u <= 0xDBFF) {
            if (i + 1 >= m) return ~0ULL;
            uint32_t v = s[2*i+2] | (uint32_t)s[2*i+3] << 8;
            if (v < 0xDC00 || v > 0xDFFF) return ~0ULL;
            cp = 0x10000 + ((u - 0xD800) << 10) + (v - 0xDC00); i += 2;
        } else if (u >= 0xDC00 && u <= 0xDFFF && !U16_LONE_LOW) return ~0ULL;
        else { cp = u; i += 1; }
        if (cp < 0x80) d[o++] = cp;
        else if (cp < 0x800) { d[o++] = 0xC0 | cp >> 6; d[o++] = 0x80 | (cp & 0x3F); }
        else if (cp < 0x10000) { d[o++] = 0xE0 | cp >> 12; d[o++] = 0x80 | ((cp >> 6) & 0x3F); d[o++] = 0x80 | (cp & 0x3F); }
        else { d[o++] = 0xF0 | cp >> 18; d[o++] = 0x80 | ((cp >> 12) & 0x3F); d[o++] = 0x80 | ((cp >> 6) & 0x3F); d[o++] = 0x80 | (cp & 0x3F); }
    }
    return o;
}
static const char T[64] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
static uint64_t b64e(uint8_t *d, const uint8_t *s, uint64_t n) {
    uint64_t i = 0, o = 0;
    for (; i + 3 <= n; i += 3) { uint32_t v = s[i] << 16 | s[i+1] << 8 | s[i+2];
        d[o++] = T[v >> 18]; d[o++] = T[(v >> 12) & 63]; d[o++] = T[(v >> 6) & 63]; d[o++] = T[v & 63]; }
    if (n - i == 1) { uint32_t v = s[i] << 16; d[o++] = T[v >> 18]; d[o++] = T[(v >> 12) & 63]; d[o++] = '='; d[o++] = '='; }
    else if (n - i == 2) { uint32_t v = s[i] << 16 | s[i+1] << 8; d[o++] = T[v >> 18]; d[o++] = T[(v >> 12) & 63]; d[o++] = T[(v >> 6) & 63]; d[o++] = '='; }
    return o;
}
static int dec(uint8_t c) {
    if (c >= 'A' && c <= 'Z') return c - 'A';
    if (c >= 'a' && c <= 'z') return c - 'a' + 26;
    if (c >= '0' && c <= '9') return c - '0' + 52;
    if (c == '+') return 62;
    if (c == '/') return 63;
    return -1;
}
static uint64_t b64d(uint8_t *d, const uint8_t *s, uint64_t n) {
    if (n & 3) return ~0ULL;
    uint64_t o = 0;
    for (uint64_t g = 0; g < n; g += 4) {
        int last = g + 4 == n;
        int a = dec(s[g]), b = dec(s[g+1]);
        if (a < 0 || b < 0) return ~0ULL;
        if (last && s[g+2] == '=' && s[g+3] == '=') {
            if (B64D_PADBITS && (b & 15)) return ~0ULL;
            d[o++] = a << 2 | b >> 4; break;
        }
        int c = dec(s[g+2]);
        if (c < 0) return ~0ULL;
        if (last && s[g+3] == '=') {
            if (B64D_PADBITS && (c & 3)) return ~0ULL;
            d[o++] = a << 2 | b >> 4; d[o++] = (b & 15) << 4 | c >> 2; break;
        }
        int e = dec(s[g+3]);
        if (e < 0) return ~0ULL;
        d[o++] = a << 2 | b >> 4; d[o++] = (b & 15) << 4 | c >> 2; d[o++] = (c & 3) << 6 | e;
    }
    return o;
}
uint64_t codec(uint64_t op, uint8_t *dst, const uint8_t *src, uint64_t n) {
    switch (op) {
    case 0: return u8to16(dst, src, n);
    case 1: return u16to8(dst, src, n);
    case 2: return b64e(dst, src, n);
    case 3: return b64d(dst, src, n);
    default: return ~0ULL;
    }
}
