.intel_syntax noprefix
.text
  mov rax, rdi
  cmp rdi, rsi
  cmovl rax, rsi
  ret
