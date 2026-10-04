.intel_syntax noprefix
.text
  mov eax, edi
  add eax, esi
  mov ecx, -1
  cmovc eax, ecx
  ret
