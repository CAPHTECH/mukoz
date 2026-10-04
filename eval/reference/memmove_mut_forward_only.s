.intel_syntax noprefix
.text
  xor ecx, ecx
4:
  cmp rcx, rdx
  jae 9f
  mov r8b, [rsi+rcx]
  mov [rdi+rcx], r8b
  inc rcx
  jmp 4b
9:
  ret
