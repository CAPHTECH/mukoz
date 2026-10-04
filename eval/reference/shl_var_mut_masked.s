.intel_syntax noprefix
.text
  mov rcx, rsi
  mov rax, rdi
  shl rax, cl
  ret
