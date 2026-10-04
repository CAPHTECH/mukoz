.intel_syntax noprefix
.text
  movzx edx, dl
  movabs r8, 0x0101010101010101
  imul rdx, r8
  movabs r9, 0x7f7f7f7f7f7f7f7f
  xor eax, eax
  xor ecx, ecx
1:
  lea r10, [rcx+8]
  cmp r10, rsi
  ja 2f
  mov r10, [rdi+rcx]
  xor r10, rdx
  mov r11, r10
  sub r11, r8
  not r10
  and r11, r10
  mov r10, r9
  not r10
  and r11, r10
  shr r11, 7
  imul r11, r8
  shr r11, 56
  add rax, r11
  add rcx, 8
  jmp 1b
2:
  cmp rcx, rsi
  jae 9f
  cmp byte ptr [rdi+rcx], dl
  jne 3f
  inc rax
3:
  inc rcx
  jmp 2b
9:
  ret
