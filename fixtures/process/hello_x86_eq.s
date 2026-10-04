# Equivalent x86-64 hello: a different instruction sequence with the same observable effect.
# Builds the text on the stack, writes it in two system calls, and leaves with exit_group.
.intel_syntax noprefix
.globl _start
_start:
  sub rsp, 16
  mov dword ptr [rsp], 0x6c6c6568      # "hell"
  mov word ptr [rsp+4], 0x0a6f         # "o\n"
  mov eax, 1
  mov edi, eax
  mov rsi, rsp
  mov edx, 2
  syscall                               # "he"
  mov eax, 1
  lea rsi, [rsp+2]
  mov edx, 4
  syscall                               # "llo\n"
  xor edi, edi
  mov eax, 231
  syscall
