add64_small_only.bin: lea rax,[rdi+rsi]; mov rcx,rdi; shr rcx,32; jz +2; xor eax,eax; ret — correct while a < 2^32 (inside add64_small's requires), returns 0 otherwise
cpuid_sig.bin: push rbx; mov eax,1; cpuid; pop rbx; ret — returns the CPUID signature, which differs between the emulator and a real CPU (differential fault injection)
