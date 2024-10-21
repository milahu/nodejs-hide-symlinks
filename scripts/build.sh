#!/bin/sh

set -ex

cargo build

realpath target/debug/libnodejs_hide_symlinks.so
