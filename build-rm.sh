#!/bin/bash

set -e

if [ -z "${RM_TOOLCHAIN+x}" ]; then
  echo "Set the RM_TOOLCHAIN env var to the toolchain path!"
  exit 1
fi

# Set up env vars from the standard rm toolchain
source $RM_TOOLCHAIN/environment-setup-cortexa7hf-neon-remarkable-linux-gnueabi

set -euo pipefail

# This is needed to cross-build with bindgen
export BINDGEN_EXTRA_CLANG_ARGS="\
  -I$RM_TOOLCHAIN/sysroots/cortexa7hf-neon-remarkable-linux-gnueabi/usr/include/c++/9.3.0\
  -I$RM_TOOLCHAIN/sysroots/cortexa7hf-neon-remarkable-linux-gnueabi/usr/include\
  -I$RM_TOOLCHAIN/sysroots/cortexa7hf-neon-remarkable-linux-gnueabi/usr/include/c++/9.3.0/arm-remarkable-linux-gnueabi"
#  --sysroot=$RM_TOOLCHAIN/sysroots/cortexa9hf-neon-oe-linux-gnueabi"

# Arguments for the tensorflow build in the build.rs file of tflite-rs.
export TFLITE_RS_MAKE_TARGET_TOOLCHAIN_PREFIX="arm-oe-linux-gnueabi-"
export TFLITE_RS_MAKE_EXTRA_CFLAGS="-march=armv7-a -mfpu=neon -mfloat-abi=hard -mcpu=cortex-a9 --sysroot=$RM_TOOLCHAIN/sysroots/cortexa9hf-neon-oe-linux-gnueabi"

exec cargo build --release --target armv7-unknown-linux-gnueabihf $@
