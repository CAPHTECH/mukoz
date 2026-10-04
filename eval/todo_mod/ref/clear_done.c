#include "common.h"
u64 clear_done(u8 *db, u64 n) {
    u64 w = 0;
    for (u64 i = 0; i < n; i++) {
        if (db[i * REC + 4]) continue;
        for (int j = 0; j < REC; j++) db[w * REC + j] = db[i * REC + j];
        w++;
    }
    return w;
}
