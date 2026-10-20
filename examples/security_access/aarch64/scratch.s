.text
.global access_probe
access_probe:
  mov x3, #127
  str x3, [sp, #-16]!
  ldr x3, [sp]
  str xzr, [sp]
  add sp, sp, #16
  ldr x3, [x1]
  str x3, [x0]
  mov x3, #0
  mov x0, #0
  ret
