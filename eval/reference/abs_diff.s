.intel_syntax noprefix
.text
  mov rax, rdi
  sub rax, rsi
  jae 1f
  neg rax
1:
  ret
