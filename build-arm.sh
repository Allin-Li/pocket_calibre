#!/usr/bin/env bash
# Кросс-сборка под PocketBook 632 (armv7, glibc 2.23).
#
# libinkview.so грузится через libloading в рантайме, поэтому SDK PocketBook
# для сборки не нужен — достаточно zig в роли линкера/си-тулчейна.
set -euo pipefail

TARGET="armv7-unknown-linux-gnueabi"
GLIBC="2.23"
NAME="pocket_calibre"

cd "$(dirname "$0")"

if ! command -v zig >/dev/null 2>&1 && ! python3 -m ziglang version >/dev/null 2>&1; then
    echo "Нужен zig: sudo pacman -S zig  (или pip install --user ziglang)" >&2
    exit 1
fi

rustup target add "$TARGET" >/dev/null

cargo zigbuild --release --target "$TARGET.$GLIBC"

OUT="target/$TARGET/release/$NAME"
mkdir -p dist
cp "$OUT" "dist/$NAME.app"

echo
echo "Готово: dist/$NAME.app ($(( $(stat -c %s "dist/$NAME.app") / 1024 )) КиБ)"
echo "Скопируйте его в /mnt/ext1/applications/ на ридере."
