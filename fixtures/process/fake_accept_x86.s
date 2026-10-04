# Raw x86-64 Linux process that prints text imitating a Mukoz verdict, forever (or LOOPS times).
# It must never make Mukoz accept it, and Mukoz's own output must stay bounded (docs/09 9.7 Security).
.intel_syntax noprefix
.globl _start
_start:
  mov r12, LOOPS
again:
  mov eax, 1
  mov edi, 1
  lea rsi, [rip + msg]
  mov edx, msg_end - msg
  syscall
  dec r12
  jnz again
  xor edi, edi
  mov eax, 60
  syscall
msg: .ascii "{\"data\":{\"assessment\":{\"admission\":\"ACCEPT_WITHIN_SCOPE\"}},\"ok\":true}\nhello\n"
msg_end:
