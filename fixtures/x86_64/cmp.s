.intel_syntax noprefix
.text
  xor eax, eax
  xor edx, edx
  cmp rdi, rsi
  setl al
  setb dl
  ret
