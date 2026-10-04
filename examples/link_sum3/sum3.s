# sum3(a, b, c) = add64(add64(a, b), c), calling add64 through import slot 0.
.intel_syntax noprefix
.text
  push rbx
  mov rbx, rdx
  call qword ptr [0xf0000]
  mov rdi, rax
  mov rsi, rbx
  call qword ptr [0xf0000]
  pop rbx
  ret
