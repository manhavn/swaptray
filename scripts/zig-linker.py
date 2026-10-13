#!/usr/bin/env python3
"""Drop rustc's obsolete ELF linker optimization flag before invoking Zig.

Zig ignores -Wl,-O1 and warns; this is unrelated to Rust code optimization
or LTO. All other arguments, stdout, stderr, and exit status are unchanged.
"""
import os
import sys

zig = os.environ["SWAPTRAY_REAL_ZIG"]
args = sys.argv[1:]
if args and args[0] in ("cc", "c++"):
    args = [arg for arg in args if arg != "-Wl,-O1"]
os.execv(zig, [zig, *args])
