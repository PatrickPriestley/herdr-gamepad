#!/bin/sh
# A development rebuild must replace the inode, not overwrite a running executable.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT HUP INT TERM
mkdir -p "$fixture/scripts" "$fixture/target/release" "$fixture/bin" "$fixture/tools"
cp "$root/scripts/build.sh" "$fixture/scripts/build.sh"
touch "$fixture/Cargo.toml"
printf '#!/bin/sh\nexit 0\n' > "$fixture/tools/cargo"
printf '#!/bin/sh\nprintf "new binary\\n"\n' > "$fixture/target/release/herd-gamepad"
printf 'old binary\n' > "$fixture/bin/herd-gamepad"
chmod +x "$fixture/tools/cargo" "$fixture/target/release/herd-gamepad"
before=$(ls -i "$fixture/bin/herd-gamepad" | awk '{print $1}')
PATH="$fixture/tools:$PATH" sh "$fixture/scripts/build.sh"
after=$(ls -i "$fixture/bin/herd-gamepad" | awk '{print $1}')
test "$before" != "$after" || { echo 'build overwrote the installed inode' >&2; exit 1; }
test "$("$fixture/bin/herd-gamepad")" = 'new binary'
echo 'atomic build installation: passed'
