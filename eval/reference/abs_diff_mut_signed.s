.intel_syntax noprefix
.text
  mov rax, rdi
  sub rax, rsi
  jge 1f
  neg rax
1:
  ret
