AArch64 fixtures: each `*.s` is assembled into `*.bin` by `build.py` (rustc's
`aarch64-unknown-linux-gnu` target with `global_asm`, then `llvm-objcopy` from the Rust toolchain;
needs `rustup target add aarch64-unknown-linux-gnu` and the `llvm-tools` component). `manifest.txt`
records sizes, SHA-256 digests and the toolchain. The expected outcome of every fixture is the
table in `tests/coverage.rs`.
