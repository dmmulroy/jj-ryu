#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
test_dir=$(mktemp -d)
trap 'rm -rf "$test_dir"' EXIT

mkdir -p \
  "$test_dir/artifacts/darwin-arm64" \
  "$test_dir/artifacts/win32-x64"
printf 'unix binary' > "$test_dir/artifacts/darwin-arm64/ryu"
printf 'windows binary' > "$test_dir/artifacts/win32-x64/ryu.exe"
chmod 644 "$test_dir/artifacts"/*/*

"$repo_root/scripts/create-release-archives.sh" \
  "$test_dir/artifacts" \
  "$test_dir/release"

unix_archive="$test_dir/release/ryu-darwin-arm64.tar.gz"
windows_archive="$test_dir/release/ryu-win32-x64.zip"

test -f "$unix_archive"
test -f "$windows_archive"
test ! -e "$test_dir/release/ryu-win32-x64.tar.gz"

tar_mode=$(tar -tvzf "$unix_archive" | awk '$NF == "ryu" { print substr($1, 1, 10) }')
test "$tar_mode" = '-rwxr-xr-x'
test "$(tar -xOzf "$unix_archive" ryu)" = 'unix binary'
test "$(unzip -p "$windows_archive" ryu.exe)" = 'windows binary'

printf 'release archive regression check passed\n'
