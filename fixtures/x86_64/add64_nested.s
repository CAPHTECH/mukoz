.intel_syntax noprefix
.text
  call helper
  ret
helper:
  lea rax, [rdi+rsi]
  ret
