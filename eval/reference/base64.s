.intel_syntax noprefix
.text
# rdi=dst rsi=src rdx=n ; standard base64, computed alphabet
  xor ecx, ecx            # i
1:
  mov r8, rdx
  sub r8, rcx             # remaining
  jz 9f
  movzx eax, byte ptr [rsi+rcx]
  shl eax, 16
  cmp r8, 1
  jbe 2f
  movzx r9d, byte ptr [rsi+rcx+1]
  shl r9d, 8
  or eax, r9d
  cmp r8, 2
  jbe 2f
  movzx r9d, byte ptr [rsi+rcx+2]
  or eax, r9d
2:
  mov r10d, 4             # 4 chars, '=' where absent
  mov r11d, eax
3:
  mov r9d, r11d
  shr r9d, 18
  and r9d, 63
  # present chars: 2 + min(remaining,3)-1
  mov edx, 4
  sub edx, r10d           # char index 0..3
  cmp rdx, r8
  ja 5f                   # index > remaining -> '='
  lea eax, [r9+65]
  cmp r9d, 26
  jb 4f
  lea eax, [r9+71]
  cmp r9d, 52
  jb 4f
  lea eax, [r9-4]
  cmp r9d, 62
  jb 4f
  mov eax, 43
  je 4f
  mov eax, 47
4:
  mov [rdi], al
  jmp 6f
5:
  mov byte ptr [rdi], 61
6:
  inc rdi
  shl r11d, 6
  dec r10d
  jnz 3b
  add rcx, 3
  mov rdx, rcx
  add rdx, r8
  sub rdx, 3              # restore n = i_old + remaining
  cmp r8, 3
  ja 1b
9:
  ret
