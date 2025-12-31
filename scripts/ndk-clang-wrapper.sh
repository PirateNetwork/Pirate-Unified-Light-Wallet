#!/usr/bin/env bash
RESOURCE_DIR="/root/android-ndk-r26d-clean/toolchains/llvm/prebuilt/linux-x86_64/lib/clang/17"
exec /lib64/ld-linux-x86-64.so.2 /root/android-ndk-r26d-clean/toolchains/llvm/prebuilt/linux-x86_64/bin/clang-17 \
  -resource-dir "$RESOURCE_DIR" \
  "$@"
