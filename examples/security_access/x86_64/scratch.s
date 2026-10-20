.intel_syntax noprefix
.text
.global access_probe
access_probe:
  mov byte ptr [rsp-8], 127
  mov al, [rsp-8]
  mov byte ptr [rsp-8], 0
  mov rax, [rsi]
  mov [rdi], rax
  xor ecx, ecx
  xor eax, eax
  ret
.section .note.GNU-stack,"",@progbits
