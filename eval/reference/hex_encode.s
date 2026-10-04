.intel_syntax noprefix
.text
  xor ecx, ecx
1:
  cmp rcx, rdx
  jae 9f
  movzx eax, byte ptr [rsi+rcx]
  mov r8d, eax
  shr r8d, 4
  and eax, 15
  lea r9d, [r8+0x30]
  cmp r8d, 10
  jb 2f
  lea r9d, [r8+0x57]
2:
  mov [rdi+rcx*2], r9b
  lea r9d, [rax+0x30]
  cmp eax, 10
  jb 3f
  lea r9d, [rax+0x57]
3:
  mov [rdi+rcx*2+1], r9b
  inc rcx
  jmp 1b
9:
  ret
