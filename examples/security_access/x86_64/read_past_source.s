.intel_syntax noprefix
.text
.global access_probe
access_probe:
  mov al, [rsi+8]
  mov rax, [rsi]
  mov [rdi], rax
  xor ecx, ecx
  xor eax, eax
  ret
.section .note.GNU-stack,"",@progbits
