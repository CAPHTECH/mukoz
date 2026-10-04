.intel_syntax noprefix
.text
  mov rcx, rdx
  mov rax, rdi
  mul rsi
  mov [rcx], rax
  jo 1f
  xor eax, eax
  ret
1:
  mov eax, 1
  ret
