use std::cell::RefCell;
use std::rc::Rc;
use unicorn_engine::{RegisterARM64, RegisterX86, Unicorn};
use unicorn_engine::unicorn_const::{Arch, HookType, MemType, Mode, Permission};

fn main() {
    // x86_64: push/pop rbx (stack write) then add; count mem accesses
    // push rbx; lea rax,[rdi+rsi]; pop rbx; ret
    let code = [0x53u8, 0x48, 0x8d, 0x04, 0x37, 0x5b, 0xc3];
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(0x10000, 0x1000, Permission::READ | Permission::EXEC).unwrap();
    uc.mem_write(0x10000, &code).unwrap();
    uc.mem_map(0x80000, 0x4000, Permission::READ | Permission::WRITE).unwrap();
    let sp = 0x84000 - 8;
    uc.mem_write(sp, &0xdead0000u64.to_le_bytes()).unwrap();
    uc.reg_write(RegisterX86::RSP, sp).unwrap();
    uc.reg_write(RegisterX86::RDI, 40).unwrap();
    uc.reg_write(RegisterX86::RSI, 2).unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    let l2 = log.clone();
    uc.add_mem_hook(HookType::MEM_READ | HookType::MEM_WRITE, 0, u64::MAX, move |_uc, t: MemType, a, s, _v| { l2.borrow_mut().push(format!("{:?} {:#x} {}", t, a, s)); true }).unwrap();
    let pcs = Rc::new(RefCell::new(Vec::new()));
    let p2 = pcs.clone();
    uc.add_code_hook(0, u64::MAX, move |_uc, a, s| p2.borrow_mut().push((a, s))).unwrap();
    let r = uc.emu_start(0x10000, 0xdead0000, 0, 1000);
    println!("x86 r={:?} rax={} pc={:#x} rsp={:#x} (expect {:#x})", r, uc.reg_read(RegisterX86::RAX).unwrap(), uc.pc_read().unwrap(), uc.reg_read(RegisterX86::RSP).unwrap(), sp + 8);
    println!("mem={:?}\npcs={:?}", log.borrow(), pcs.borrow());

    // x86_64: out-of-bounds write -> error
    let bad = [0x48u8, 0x89, 0x38, 0xc3]; // mov [rax],rdi; ret
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(0x10000, 0x1000, Permission::READ | Permission::EXEC).unwrap();
    uc.mem_write(0x10000, &bad).unwrap();
    uc.reg_write(RegisterX86::RAX, 0x500000).unwrap();
    let r = uc.emu_start(0x10000, 0xdead0000, 0, 1000);
    println!("oob r={:?} pc={:#x}", r, uc.pc_read().unwrap());

    // invalid instruction (ud2)
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(0x10000, 0x1000, Permission::READ | Permission::EXEC).unwrap();
    uc.mem_write(0x10000, &[0x0f, 0x0b]).unwrap();
    println!("ud2 r={:?}", uc.emu_start(0x10000, 0xdead0000, 0, 1000));

    // syscall instruction -> intr hook?
    let mut uc = Unicorn::new(Arch::X86, Mode::MODE_64).unwrap();
    uc.mem_map(0x10000, 0x1000, Permission::READ | Permission::EXEC).unwrap();
    uc.mem_write(0x10000, &[0x0f, 0x05, 0xc3]).unwrap();
    uc.add_insn_sys_hook(unicorn_engine::InsnSysX86::SYSCALL, 0, u64::MAX, |uc| { println!("  syscall hook rax={}", uc.reg_read(RegisterX86::RAX).unwrap()); uc.emu_stop().unwrap(); }).unwrap();
    uc.reg_write(RegisterX86::RAX, 60).unwrap();
    println!("syscall r={:?}", uc.emu_start(0x10000, 0xdead0000, 0, 1000));

    // aarch64 add x0,x0,x1; ret
    let a64 = [0x00u8, 0x00, 0x01, 0x8b, 0xc0, 0x03, 0x5f, 0xd6];
    let mut uc = Unicorn::new(Arch::ARM64, Mode::ARM).unwrap();
    uc.mem_map(0x10000, 0x1000, Permission::READ | Permission::EXEC).unwrap();
    uc.mem_write(0x10000, &a64).unwrap();
    uc.mem_map(0x80000, 0x4000, Permission::READ | Permission::WRITE).unwrap();
    uc.reg_write(RegisterARM64::SP, 0x84000).unwrap();
    uc.reg_write(RegisterARM64::X30, 0xdead0000).unwrap();
    uc.reg_write(RegisterARM64::X0, u64::MAX).unwrap();
    uc.reg_write(RegisterARM64::X1, 2).unwrap();
    let r = uc.emu_start(0x10000, 0xdead0000, 0, 1000);
    println!("a64 r={:?} x0={:#x} pc={:#x}", r, uc.reg_read(RegisterARM64::X0).unwrap(), uc.pc_read().unwrap());
    // aarch64 sub x0,x0,x1 = 00 00 01 cb
    uc.mem_write(0x10000, &[0x00, 0x00, 0x01, 0xcb]).unwrap();
    uc.reg_write(RegisterARM64::X30, 0xdead0000).unwrap();
    uc.reg_write(RegisterARM64::X0, 5).unwrap();
    uc.reg_write(RegisterARM64::X1, 7).unwrap();
    let r = uc.emu_start(0x10000, 0xdead0000, 0, 1000);
    println!("a64 sub r={:?} x0={:#x}", r, uc.reg_read(RegisterARM64::X0).unwrap());
}
