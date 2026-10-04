"""Assemble fixtures/aarch64/*.s into raw .bin files with rustc's aarch64 target (global_asm) and
llvm-objcopy, and write manifest.txt (size, sha256, toolchain). Lines starting with // are comments."""
import glob, hashlib, os, subprocess, tempfile
os.chdir(os.path.dirname(os.path.abspath(__file__)))
OBJCOPY = glob.glob(os.path.expanduser('~/.rustup/toolchains/*/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-objcopy'))[0]
rustc = subprocess.check_output(['rustc', '--version'], text=True).strip()
lines_out = [f'# toolchain: {rustc} (aarch64-unknown-linux-gnu global_asm), llvm-objcopy']
for src in sorted(glob.glob('*.s')):
    body = [l.strip() for l in open(src) if l.strip() and not l.strip().startswith('//')]
    with tempfile.TemporaryDirectory() as d:
        open(f'{d}/t.rs', 'w').write('#![no_std]\ncore::arch::global_asm!(' + ', '.join('"%s"' % l for l in body) + ');\n')
        subprocess.check_call(['rustc', '--target', 'aarch64-unknown-linux-gnu', '--crate-type=lib', '--emit=obj', '-C', 'panic=abort', '-o', f'{d}/t.o', f'{d}/t.rs'])
        subprocess.check_call([OBJCOPY, '-O', 'binary', '-j', '.text', f'{d}/t.o', f'{d}/t.bin'])
        b = open(f'{d}/t.bin', 'rb').read()
    out = src[:-2] + '.bin'
    open(out, 'wb').write(b)
    lines_out.append(f'{out} {len(b)} {hashlib.sha256(b).hexdigest()}')
open('manifest.txt', 'w').write('\n'.join(lines_out) + '\n')
print('\n'.join(lines_out))
