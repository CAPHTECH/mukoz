.intel_syntax noprefix
.text
  mov [rdi], rsi
  lea rax, [rdi+rsi]
  ret
