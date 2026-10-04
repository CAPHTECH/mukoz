.intel_syntax noprefix
.text
  mov rcx, rdx
  shr rcx, 1
  xor eax, eax
1:
  cmp rax, rcx
  jae 2f
  mov r8b, [rsi+rax]
  mov [rdi+rax], r8b
  inc rax
  jmp 1b
2:
  ret
