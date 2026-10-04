#include "common.h"
u64 del_rec(u8 *db, u64 n, u64 idx) {
    for (u64 i = idx * REC; i < (n - 1) * REC; i++) db[i] = db[i + REC];
    return n - 1;
}
