#!/usr/bin/env python3
"""Cross-platform zip extractor tolerant of backslash separators.

Some upstream WebUI release zips use backslash separators in entry names,
which breaks unzip/Expand-Archive/tar on Windows. This normalizes '\\' to '/'
while extracting (same trick the manual staging flow used).
Usage: python unzip_any.py <zip> <dst>
"""
import os
import sys
import zipfile


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: unzip_any.py <zip> <dst>", file=sys.stderr)
        return 1
    src, dst = sys.argv[1], sys.argv[2]
    os.makedirs(dst, exist_ok=True)
    with zipfile.ZipFile(src) as z:
        for member in z.namelist():
            path = member.replace("\\", "/")
            target = os.path.join(dst, path)
            if path.endswith("/"):
                os.makedirs(target, exist_ok=True)
                continue
            os.makedirs(os.path.dirname(target), exist_ok=True)
            with z.open(member) as f, open(target, "wb") as out:
                out.write(f.read())
    return 0


if __name__ == "__main__":
    sys.exit(main())
