.intel_syntax noprefix
.text
  xor eax, eax
  cmp rsi, 63
  ja 1f
  mov rcx, rsi
  mov rax, rdi
  shl rax, cl
1:
  ret
