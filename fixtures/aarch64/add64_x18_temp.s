// Uses x18 as a scratch register and restores it: allowed by aapcs64 (Linux), not by apple-arm64.
mov x9, x18
mov x18, x1
add x0, x0, x18
mov x18, x9
ret
