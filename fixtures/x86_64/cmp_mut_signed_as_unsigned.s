.intel_syntax noprefix
.text
  # signed result computed with an unsigned condition
  xor eax, eax
  xor edx, edx
  cmp rdi, rsi
  setb al
  setb dl
  ret
