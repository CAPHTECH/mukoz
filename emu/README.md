# mukoz-emu

An optional machine emulator program for Mukoz's `emulated` executor. It wraps
[Unicorn](https://github.com/unicorn-engine/unicorn) 2.1.1 and speaks the line protocol in
[PROTOCOL.md](PROTOCOL.md). It does not contain or link any part of `mukoz`, and `mukoz` does not
link it: `mukoz` starts it as a child process when `$MUKOZ_EMU` names it. The default program is
`mukoz-emu-icicle` (../emu-icicle/), which contains no GPL code. This one is built only on request:
`cargo build --release -p mukoz-emu` (needs CMake and a C compiler).

## License

mukoz-emu is distributed under the GNU General Public License, version 2 or (at your option)
any later version (`GPL-2.0-or-later`), because it links Unicorn, which is distributed under the
GPL version 2. The full text is in [LICENSE](LICENSE).

`mukoz` itself, in the parent directory, is under `MIT OR Apache-2.0`.

Copyright (c) 2026 The Mukoz authors.
