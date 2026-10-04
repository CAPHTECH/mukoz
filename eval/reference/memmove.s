.intel_syntax noprefix
.text
  cmp rdi, rsi
  jbe 3f
  mov rcx, rdx
1:
  test rcx, rcx
  jz 9f
  dec rcx
  mov r8b, [rsi+rcx]
  mov [rdi+rcx], r8b
  jmp 1b
3:
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
