.intel_syntax noprefix
.text
  xor eax, eax
1:
  cmp rax, rdx
  jae 2f
  mov cl, [rsi+rax]
  mov [rdi+rax], cl
  inc rax
  jmp 1b
2:
  ret
