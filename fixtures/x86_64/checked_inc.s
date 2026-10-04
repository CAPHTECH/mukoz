.intel_syntax noprefix
.text
  mov rax, [rdi]
  add rax, rsi
  jc 1f
  mov [rdi], rax
  xor eax, eax
  ret
1:
  mov eax, 1
  ret
