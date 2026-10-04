typedef unsigned long u64;
typedef unsigned int u32;
typedef unsigned char u8;
#define REC 72
#define SLOT(k) (*(u64 *)(0xf0000UL + 8 * (k)))
