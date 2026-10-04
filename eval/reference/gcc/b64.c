#include <stdint.h>
#include <stddef.h>
static const char T[64] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
void b64(uint8_t *dst, const uint8_t *src, uint64_t n) {
    size_t i = 0;
    for (; i + 3 <= n; i += 3) {
        uint32_t v = (uint32_t)src[i] << 16 | (uint32_t)src[i + 1] << 8 | src[i + 2];
        *dst++ = T[v >> 18]; *dst++ = T[(v >> 12) & 63]; *dst++ = T[(v >> 6) & 63]; *dst++ = T[v & 63];
    }
    size_t r = n - i;
    if (r == 1) {
        uint32_t v = (uint32_t)src[i] << 16;
        dst[0] = T[v >> 18]; dst[1] = T[(v >> 12) & 63]; dst[2] = '='; dst[3] = '=';
    } else if (r == 2) {
        uint32_t v = (uint32_t)src[i] << 16 | (uint32_t)src[i + 1] << 8;
        dst[0] = T[v >> 18]; dst[1] = T[(v >> 12) & 63]; dst[2] = T[(v >> 6) & 63]; dst[3] = '=';
    }
}
