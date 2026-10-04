.intel_syntax noprefix
.text
  xor eax, eax
1:
  cmp byte ptr [rdi+rax], 0
  je 2f
  inc rax
  jmp 1b
2:
  ret
