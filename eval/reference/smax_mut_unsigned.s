.intel_syntax noprefix
.text
  mov rax, rdi
  cmp rdi, rsi
  cmovb rax, rsi
  ret
