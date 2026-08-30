#!/usr/bin/env python3
"""One-off migration: rewrite dual-backend `match &self.db { Db::Sqlite(..).., Db::Postgres(..).. }`
blocks into single PostgreSQL expressions, and strip `db_type()` args from bind_placeholders.
Operates on suwayomi-domain src files."""
import re
from pathlib import Path

FILES = list(Path("crates/suwayomi-domain/src").rglob("*.rs"))


def find_matching(src, open_idx):
    """index of '{' -> index of matching '}', skipping string literals"""
    depth = 0
    i = open_idx
    n = len(src)
    while i < n:
        c = src[i]
        if c == '"' or c == "'":
            quote = c
            i += 1
            while i < n:
                if src[i] == "\\" and quote == '"':
                    i += 2
                    continue
                if src[i] == quote:
                    if quote == "'" and i + 1 < n and src[i + 1] == "'":
                        i += 2
                        continue
                    break
                i += 1
        elif c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    raise ValueError("unbalanced")


def split_arms(body):
    """split match body at top-level commas OR at top-level '}' followed by
    the next arm pattern (Rust allows omitting the comma after a block arm)."""
    arms = []
    depth = 0
    gen_depth = 0
    start = 0
    i = 0
    n = len(body)
    while i < n:
        c = body[i]
        if c == '"' or c == "'":
            quote = c
            i += 1
            while i < n:
                if body[i] == "\\" and quote == '"':
                    i += 2
                    continue
                if body[i] == quote:
                    if quote == "'" and i + 1 < n and body[i + 1] == "'":
                        i += 2
                        continue
                    break
                i += 1
        elif c in "({[":
            depth += 1
        elif c in ")}]":
            depth -= 1
            if depth == 0 and c == "}":
                j = i + 1
                while j < n and body[j] in " \t\n\r":
                    j += 1
                if body[j:j + 3] == "Db:":
                    arms.append(body[start:i + 1])
                    start = j
                    i = j - 1
        elif c == "<" and depth == 0:
            gen_depth += 1
        elif c == ">" and depth == 0 and gen_depth > 0:
            gen_depth -= 1
        elif c == "," and depth == 0 and gen_depth == 0:
            arms.append(body[start:i])
            start = i + 1
        i += 1
    tail = body[start:]
    if tail.strip():
        arms.append(tail)
    return arms


def extract_arm(arm):
    """return (pat, expr) from 'pat => expr'"""
    m = re.match(r"\s*(Db::\w+(?:\([^)]*\))?)\s*=>\s*(.*)$", arm, re.S)
    if not m:
        return None
    return m.group(1).strip(), m.group(2).strip()


def fix_expr(expr):
    return re.sub(r"\bpool\b", "self.db.pool()", expr)


def process(src):
    out = []
    i = 0
    n = len(src)
    while i < n:
        m = re.search(r"match\s+(?:&\s*)?(?:self\.)?db\s*\{", src[i:])
        if not m:
            out.append(src[i:])
            break
        start = i + m.start()
        out.append(src[i:start])
        open_idx = i + m.end() - 1
        close_idx = find_matching(src, open_idx)
        body = src[open_idx + 1:close_idx]
        arms = split_arms(body)
        exprs = []
        ok = True
        for arm in arms:
            parsed = extract_arm(arm)
            if not parsed:
                ok = False
                break
            pat, expr = parsed
            if pat.startswith("Db::Sqlite") or pat.startswith("Db::Postgres"):
                exprs.append(fix_expr(expr))
            else:
                ok = False
                break
        if ok and len(exprs) == 2:
            chosen = exprs[1] if exprs[0] != exprs[1] else exprs[0]
            out.append(chosen)
        else:
            out.append(src[start:close_idx + 1])
        i = close_idx + 1
    return "".join(out)


def strip_db_type_args(src):
    # handles: bind_placeholders(<sql>,\n    self.db.db_type(),\n);
    src = re.sub(
        r"bind_placeholders\((.*?)\s*,\s*self\.db\.db_type\(\)\s*,?\s*\)",
        r"bind_placeholders(\1)",
        src,
        flags=re.S,
    )
    src = re.sub(
        r"bind_placeholders\((.*?)\s*,\s*db_type\s*,?\s*\)",
        r"bind_placeholders(\1)",
        src,
        flags=re.S,
    )
    return src


def main():
    for f in FILES:
        src = f.read_text(encoding="utf-8")
        orig = src
        src = strip_db_type_args(src)
        src = process(src)
        if src != orig:
            f.write_text(src, encoding="utf-8")
            print("rewrote", f)


if __name__ == "__main__":
    main()
