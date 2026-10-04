.intel_syntax noprefix
.text
# Bug only on the misaligned-destination path: copies one byte too few.
  xor eax, eax
  test dil, 7
  jz 1f
  test rdx, rdx
  jz 2f
  dec rdx
1:
  cmp rax, rdx
  jae 2f
  mov cl, [rsi+rax]
  mov [rdi+rax], cl
  inc rax
  jmp 1b
2:
  ret
