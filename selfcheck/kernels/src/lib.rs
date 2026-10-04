//! Kernels of Mukoz (docs/09 9.6 stage 3): `no_std`, no allocation, no panics, `extern "C"`
//! entry points. Mukoz links them for its own use (the memory monitor calls
//! `mk_range_contains`), and `selfcheck/stage3` compiles the same source to x86_64 and aarch64
//! object files and checks each function as a routine with Mukoz.
#![no_std]

/// 64-bit wrapping addition.
#[unsafe(no_mangle)]
pub extern "C" fn mk_bv_add64(a: u64, b: u64) -> u64 {
    a.wrapping_add(b)
}

/// Unsigned less-than (1 or 0).
#[unsafe(no_mangle)]
pub extern "C" fn mk_bv_ult(a: u64, b: u64) -> u64 {
    (a < b) as u64
}

/// Signed less-than (1 or 0).
#[unsafe(no_mangle)]
pub extern "C" fn mk_bv_slt(a: u64, b: u64) -> u64 {
    ((a as i64) < (b as i64)) as u64
}

/// Shift left with bit-vector semantics: a count of 64 or more gives 0.
#[unsafe(no_mangle)]
pub extern "C" fn mk_bv_shl(a: u64, n: u64) -> u64 {
    if n >= 64 { 0 } else { a << n }
}

/// Sign-extend the low `w` bits of `x` (w = 0 or w >= 64 returns `x`).
#[unsafe(no_mangle)]
pub extern "C" fn mk_bv_sext(x: u64, w: u64) -> u64 {
    if w == 0 || w >= 64 {
        return x;
    }
    let s = 64 - w;
    (((x << s) as i64) >> s) as u64
}

/// Is `[addr, addr + width)` inside `[base, base + size)`? Wrapping ranges are never inside.
/// A zero-width access is inside any range that contains `addr` or ends at it.
#[unsafe(no_mangle)]
pub extern "C" fn mk_range_contains(base: u64, size: u64, addr: u64, width: u64) -> u64 {
    let Some(end) = addr.checked_add(width) else { return 0 };
    let Some(limit) = base.checked_add(size) else { return 0 };
    (addr >= base && end <= limit) as u64
}

/// Result codes of `mk_elf_header_check`.
pub mod elf {
    pub const OK: u64 = 0;
    pub const SHORT: u64 = 1;
    pub const MAGIC: u64 = 2;
    pub const CLASS: u64 = 3;
    pub const DATA: u64 = 4;
    pub const TYPE: u64 = 5;
    pub const MACHINE: u64 = 6;
    pub const PHENTSIZE: u64 = 7;
    pub const PH_OUTSIDE: u64 = 8;
}

#[inline(always)]
unsafe fn rd(p: *const u8, off: usize, n: usize) -> u64 {
    let mut v = 0u64;
    let mut i = 0;
    while i < n {
        v |= (unsafe { *p.add(off + i) } as u64) << (8 * i);
        i += 1;
    }
    v
}

/// Check an ELF64 little-endian executable header in `buf[..len]` for `machine`
/// (62 = x86_64, 183 = aarch64): magic, class, data, type (ET_EXEC or ET_DYN), machine,
/// program header entry size and that the program header table lies inside `len`.
///
/// # Safety
/// `buf` must be readable for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mk_elf_header_check(buf: *const u8, len: u64, machine: u64) -> u64 {
    if len < 64 {
        return elf::SHORT;
    }
    let r = |off: usize, n: usize| unsafe { rd(buf, off, n) };
    if r(0, 4) != 0x464c_457f {
        return elf::MAGIC;
    }
    if r(4, 1) != 2 {
        return elf::CLASS;
    }
    if r(5, 1) != 1 {
        return elf::DATA;
    }
    let ty = r(16, 2);
    if ty != 2 && ty != 3 {
        return elf::TYPE;
    }
    if r(18, 2) != machine {
        return elf::MACHINE;
    }
    let phoff = r(32, 8);
    let phentsize = r(54, 2);
    let phnum = r(56, 2);
    if phentsize < 56 {
        return elf::PHENTSIZE;
    }
    // phoff + phentsize * phnum <= len, without overflow (phentsize, phnum < 2^16).
    let table = phentsize * phnum;
    match phoff.checked_add(table) {
        Some(end) if end <= len => elf::OK,
        _ => elf::PH_OUTSIDE,
    }
}

/// Copy `n` bytes. The compiler turns this into a `memcpy` call: checked as a routine it must be
/// reported as UNRESOLVED_DEPENDENCY, never judged (docs/09 9.6).
///
/// # Safety
/// Standard `copy_nonoverlapping` requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mk_copy_bytes(dst: *mut u8, src: *const u8, n: u64) {
    unsafe { core::ptr::copy_nonoverlapping(src, dst, n as usize) }
}
