# copy_twice(dst, src, n): copy(dst, src, n); copy(dst + n, src, n) through import slot 0.
.intel_syntax noprefix
.text
  push rbx
  push r12
  push r13
  mov rbx, rdi
  mov r12, rsi
  mov r13, rdx
  call qword ptr [0xf0000]
  lea rdi, [rbx + r13]
  mov rsi, r12
  mov rdx, r13
  call qword ptr [0xf0000]
  pop r13
  pop r12
  pop rbx
  ret
