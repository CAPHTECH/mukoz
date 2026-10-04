.intel_syntax noprefix
.text
  test rsi, rsi
  jz 2f
  lea rcx, [rdi+rsi-1]
1:
  cmp rdi, rcx
  jae 2f
  mov al, [rdi]
  mov dl, [rcx]
  mov [rdi], dl
  mov [rcx], al
  inc rdi
  dec rcx
  jmp 1b
2:
  ret
