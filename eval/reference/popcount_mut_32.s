.intel_syntax noprefix
.text
  xor eax, eax
1:
  test edi, edi
  jz 2f
  mov ecx, edi
  and ecx, 1
  add rax, rcx
  shr edi, 1
  jmp 1b
2:
  ret
