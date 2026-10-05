# mukoz-emu

The machine emulator program behind Mukoz's `emulated` executor. It wraps
[Unicorn](https://github.com/unicorn-engine/unicorn) 2.1.1 and speaks the line protocol in
[PROTOCOL.md](PROTOCOL.md). It does not contain or link any part of `mukoz`, and `mukoz` does not
link it: `mukoz` starts it as a child process (next to the `mukoz` executable, or `$MUKOZ_EMU`).

## License

mukoz-emu is distributed under the GNU General Public License, version 2 or (at your option)
any later version (`GPL-2.0-or-later`), because it links Unicorn, which is distributed under the
GPL version 2. The full text is in [LICENSE](LICENSE).

`mukoz` itself, in the parent directory, is under `MIT OR Apache-2.0`.

Copyright (c) 2026 The Mukoz authors.
