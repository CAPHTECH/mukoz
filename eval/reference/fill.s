.intel_syntax noprefix
.text
  xor eax, eax
1:
  cmp rax, rdx
  jae 2f
  mov [rdi+rax], sil
  inc rax
  jmp 1b
2:
  ret
