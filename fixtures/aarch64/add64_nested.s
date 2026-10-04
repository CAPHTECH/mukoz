// The first ret belongs to the helper: the check must not stop there.
stp x29, x30, [sp, #-16]!
mov x29, sp
bl 1f
ldp x29, x30, [sp], #16
ret
1:
add x0, x0, x1
ret
