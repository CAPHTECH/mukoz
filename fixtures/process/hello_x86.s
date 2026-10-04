# Raw x86-64 Linux process: write "hello\n" to stdout, exit 0.
.intel_syntax noprefix
.globl _start
_start:
  mov eax, 1
  mov edi, 1
  lea rsi, [rip + msg]
  mov edx, LEN
  syscall
  xor edi, edi
  mov eax, 60
  syscall
msg: .ascii "hello\n"
