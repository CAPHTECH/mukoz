// Copies one byte too many (b.hi instead of b.hs).
mov x3, #0
1:
cmp x3, x2
b.hi 2f
ldrb w4, [x1, x3]
strb w4, [x0, x3]
add x3, x3, #1
b 1b
2:
ret
