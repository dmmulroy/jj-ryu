#!/usr/bin/env bash
set -euo pipefail

artifacts_dir=${1:-artifacts}
release_dir=${2:-release}

mkdir -p "$release_dir"
artifacts_dir=$(cd "$artifacts_dir" && pwd)
release_dir=$(cd "$release_dir" && pwd)

for platform_dir in "$artifacts_dir"/*/; do
  platform=$(basename "$platform_dir")

  case "$platform" in
    win32-*)
      (cd "$platform_dir" && zip -q "$release_dir/ryu-${platform}.zip" ryu.exe)
      ;;
    *)
      # actions/upload-artifact does not preserve file permissions.
      chmod 755 "$platform_dir/ryu"
      (cd "$platform_dir" && tar -czf "$release_dir/ryu-${platform}.tar.gz" ryu)
      ;;
  esac
done
