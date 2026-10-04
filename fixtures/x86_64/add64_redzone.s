.intel_syntax noprefix
.text
  mov [rsp-8], rdi
  mov rax, [rsp-8]
  add rax, rsi
  ret
