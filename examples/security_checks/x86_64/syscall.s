.intel_syntax noprefix
.text
mov eax, 39
syscall
lea rax, [rdi+rsi]
ret
