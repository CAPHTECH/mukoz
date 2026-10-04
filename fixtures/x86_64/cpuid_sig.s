.intel_syntax noprefix
.text
  # Returns a value the emulator and the host CPU disagree on (the CPUID leaf 1 signature:
  # family, model, stepping): the differential test must report BACKEND_DIVERGENCE.
  push rbx
  mov eax, 1
  cpuid
  pop rbx
  ret
