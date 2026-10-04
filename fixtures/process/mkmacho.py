"""AArch64 Mach-O hello fixtures (no Apple toolchain on the build host).

Code is assembled with rustc's AArch64 target (global_asm) and llvm-objcopy; the Mach-O header
is written here: __PAGEZERO, __TEXT (one __text section), __LINKEDIT, LC_LOAD_DYLINKER, LC_MAIN,
LC_BUILD_VERSION (ncmds = 6, sizeofcmds = 376, code at file offset 0x300, like the 0.4 record).
Whether a real Mac runs these files is not checked [U]: no dylib is linked."""
import glob, os, struct, subprocess, tempfile

OBJCOPY = glob.glob(os.path.expanduser('~/.rustup/toolchains/*/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-objcopy'))[0]

def asm(lines):
    with tempfile.TemporaryDirectory() as d:
        open(f'{d}/t.rs', 'w').write('#![no_std]\ncore::arch::global_asm!(' + ', '.join('"%s"' % l for l in lines) + ');\n')
        subprocess.check_call(['rustc', '--target', 'aarch64-unknown-linux-gnu', '--crate-type=lib', '--emit=obj', '-C', 'panic=abort', '-o', f'{d}/t.o', f'{d}/t.rs'])
        subprocess.check_call([OBJCOPY, '-O', 'binary', '-j', '.text', f'{d}/t.o', f'{d}/t.bin'])
        return open(f'{d}/t.bin', 'rb').read()

def hello(n, tail):
    # main(argc, argv, envp, apple): write(1, msg, n) through darwin svc #0x80, then `tail`.
    return asm(['mov x0, #1', 'adr x1, 2f', f'mov x2, #{n}', 'mov x16, #4', 'svc #0x80'] + tail + ['2:', '.ascii \\"hello\\\\n\\"'])

RET0 = ['mov x0, #0', 'ret']                     # return 0 from main
EXIT0 = ['mov x0, #0', 'mov x16, #1', 'svc #0x80']  # exit(0) system call: equivalent behaviour

def seg(name, vmaddr, vmsize, fileoff, filesize, prot, sects=b'', nsects=0):
    body = struct.pack('<16sQQQQiiII', name.encode(), vmaddr, vmsize, fileoff, filesize, prot, prot, nsects, 0) + sects
    return struct.pack('<II', 0x19, 8 + len(body)) + body

def macho(code, extra_cmds=b'', extra_n=0):
    text_vm, code_off, size = 0x100000000, 0x300, 0x4000
    sect = struct.pack('<16s16sQQIIIIIIII', b'__text', b'__TEXT', text_vm + code_off, len(code), code_off, 2, 0, 0, 0x80000400, 0, 0, 0)
    dylinker = struct.pack('<III', 0xe, 32, 12) + b'/usr/lib/dyld'.ljust(20, b'\0')
    cmds = (seg('__PAGEZERO', 0, 0x100000000, 0, 0, 0)
            + seg('__TEXT', text_vm, size, 0, size, 5, sect, 1)
            + seg('__LINKEDIT', text_vm + size, size, size, 0, 1)
            + dylinker
            + struct.pack('<IIQQ', 0x80000028, 24, code_off, 0)
            + struct.pack('<IIIIII', 0x32, 24, 1, 0x000b0000, 0x000b0000, 0)
            + extra_cmds)
    hdr = struct.pack('<IiiIIIII', 0xfeedfacf, 0x0100000c, 0, 2, 6 + extra_n, len(cmds), 0x00200085, 0)
    out = bytearray(size)
    out[:len(hdr) + len(cmds)] = hdr + cmds
    out[code_off:code_off + len(code)] = code
    return bytes(out)

good = macho(hello(6, RET0))
open('hello_a64.macho', 'wb').write(good)
open('hello_a64_eq.macho', 'wb').write(macho(hello(6, EXIT0)))
open('hello_a64_mut_len.macho', 'wb').write(macho(hello(7, RET0)))
open('hello_a64_mut_x18.macho', 'wb').write(macho(hello(6, ['mov x18, #1'] + RET0)))  # x18 is reserved on Apple platforms
dylib = struct.pack('<IIIIII', 0xc, 56, 24, 2, 0x10000, 0x10000) + b'/usr/lib/libSystem.B.dylib'.ljust(32, b'\0')
open('hello_a64_dylib.macho', 'wb').write(macho(hello(6, RET0), dylib, 1))
def put(b, off, fmt, v):
    b = bytearray(b); struct.pack_into(fmt, b, off, v); return bytes(b)
open('broken_truncated.macho', 'wb').write(good[:100])
open('broken_cmdsize.macho', 'wb').write(put(good, 32 + 72 + 4, '<I', 0))             # __TEXT cmdsize = 0
open('broken_fileoff.macho', 'wb').write(put(good, 32 + 72 + 40, '<Q', 0xfffffffffffff000))  # __TEXT fileoff
open('broken_ncmds.macho', 'wb').write(put(good, 16, '<I', 0xffffffff))
