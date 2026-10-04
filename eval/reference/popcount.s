.intel_syntax noprefix
.text
  xor eax, eax
1:
  test rdi, rdi
  jz 2f
  mov rcx, rdi
  and rcx, 1
  add rax, rcx
  shr rdi, 1
  jmp 1b
2:
  ret
