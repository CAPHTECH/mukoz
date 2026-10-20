.text
.global access_probe
access_probe:
  ldr x3, [x1]
  str x3, [x0]
  mov x3, #0
  mov x0, #0
  ret
