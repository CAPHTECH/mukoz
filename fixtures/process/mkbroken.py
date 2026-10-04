"""Broken ELF headers derived from hello_x86.elf (docs/09 9.7: header, segment and integer-overflow bounds)."""
import struct
good = open("hello_x86.elf", "rb").read()
def put(b, off, fmt, v):
    b = bytearray(b); struct.pack_into(fmt, b, off, v); return bytes(b)
open("broken_truncated.elf", "wb").write(good[:40])
open("broken_phoff.elf", "wb").write(put(good, 32, "<Q", 0xffffffffffffff00))
phoff = struct.unpack_from("<Q", good, 32)[0]
open("broken_filesz.elf", "wb").write(put(good, phoff + 32, "<Q", 0xfffffffffffff000))
open("broken_class.elf", "wb").write(put(good, 4, "<B", 1))
