#!/bin/sh
# Self-check stage 2 (docs/09 9.6): build a static mukoz (the subject) into bin/, then write the
# contract, binding and suite (gen.py). Static linking: the native-process sandbox has no loader.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root"
RUSTFLAGS="-C target-feature=+crt-static" \
  cargo build --release -p mukoz --target x86_64-unknown-linux-gnu --target-dir target/static
mkdir -p "$here/bin"
cp target/static/x86_64-unknown-linux-gnu/release/mukoz "$here/bin/mukoz-static"
{ echo "# $(rustc --version); RUSTFLAGS=-C target-feature=+crt-static"; echo "bin/mukoz-static $(wc -c < "$here/bin/mukoz-static") $(sha256sum "$here/bin/mukoz-static" | cut -d' ' -f1)"; } > "$here/bin/manifest.txt"
python3 "$here/gen.py"
cat "$here/bin/manifest.txt"
