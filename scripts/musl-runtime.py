#!/usr/bin/env python3
"""Fetch a checksum-pinned musl shared loader for cross linking (not packaged)."""
import hashlib
import io
import pathlib
import sys
import tarfile
import urllib.request

arch, output = sys.argv[1:]
checksums = {
    "x86_64": "4990a5e0ba312e478f94cfe431a70efef1538004eb361c8ae424516848be45bb",
    "aarch64": "ac281d1e7f9e9c447c51e309317b975f48be6edaf3ab91ae73b959cf86703782",
}
data = urllib.request.urlopen(
    f"https://dl-cdn.alpinelinux.org/alpine/v3.22/main/{arch}/musl-1.2.5-r12.apk",
    timeout=60,
).read()
if hashlib.sha256(data).hexdigest() != checksums[arch]:
    raise SystemExit("musl package checksum mismatch")
with tarfile.open(fileobj=io.BytesIO(data)) as archive:
    loader = archive.extractfile(f"lib/ld-musl-{arch}.so.1").read()
pathlib.Path(output).write_bytes(loader)
