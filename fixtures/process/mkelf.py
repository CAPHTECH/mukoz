"""AArch64 hello (hand-encoded) as a raw file and as a minimal static ELF."""
import struct
W = [
    0xD2800808,  # movz x8, #64          (write)
    0xD2800020,  # movz x0, #1
    0x100000C1,  # adr  x1, msg (+24)
    0xD28000C2,  # movz x2, #6
    0xD4000001,  # svc  #0
    0xD2800000,  # movz x0, #0
    0xD2800BA8,  # movz x8, #93          (exit)
    0xD4000001,  # svc  #0
]
code = b"".join(struct.pack("<I", w) for w in W) + b"hello\n\0\0"
open("hello_a64.bin", "wb").write(code)
base, off = 0x400000, 64 + 56
ehdr = b"\x7fELF" + bytes([2, 1, 1, 0]) + bytes(8) + struct.pack("<HHIQQQIHHHHHH", 2, 183, 1, base + off, 64, 0, 0, 64, 56, 1, 64, 0, 0)
phdr = struct.pack("<IIQQQQQQ", 1, 5, 0, base, base, off + len(code), off + len(code), 0x1000)
open("hello_a64.elf", "wb").write(ehdr + phdr + code)
