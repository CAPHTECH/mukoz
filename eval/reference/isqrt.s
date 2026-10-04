.intel_syntax noprefix
.text
  xor eax, eax
  mov ecx, 31
1:
  mov r8, rax
  bts r8, rcx
  mov r9, r8
  imul r9, r8
  cmp r9, rdi
  ja 2f
  mov rax, r8
2:
  dec ecx
  jns 1b
  ret
