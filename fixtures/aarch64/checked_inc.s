ldr x2, [x0]
adds x2, x2, x1
b.cs 1f
str x2, [x0]
mov w0, #0
ret
1:
mov w0, #1
ret
