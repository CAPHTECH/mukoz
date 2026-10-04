.intel_syntax noprefix
.text
  xor eax, eax
  xor ecx, ecx
1:
  cmp rcx, rsi
  jae 2f
  cmp [rdi+rcx], dl
  jne 3f
  inc rax
3:
  inc rcx
  jmp 1b
2:
  ret
