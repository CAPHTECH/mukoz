.intel_syntax noprefix
.text
  push rbx
  mov rbx, rdi
  lea rax, [rbx+rsi]
  pop rbx
  ret
