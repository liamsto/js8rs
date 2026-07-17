#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

import argparse
import os
import re
import struct
from dataclasses import dataclass
from typing import List, Tuple as PyTuple


# Matches one entry per line, with optional trailing comma and optional C-style comments
# Example accepted:
#   {"\xbf" /* ¿ */, 1, 239517},
#   {"~", 1, 66},
ENTRY_RE = re.compile(
    r"""
    ^\s*\{\s*"
        (?P<raw>(?:\\.|[^"\\])*)
    "\s*
        (?:
            (?:/\*.*?\*/\s*)      # optional /* ... */ comment
          | (?://[^\n]*\s*)       # optional // ... comment
        )*
    ,\s*(?P<size>[+-]?\d+)\s*,\s*(?P<index>[+-]?\d+)\s*
    \}\s*,?\s*$
    """,
    re.VERBOSE,
)


def c_unescape(s: str) -> str:
    out = []
    i = 0
    n = len(s)
    while i < n:
        ch = s[i]
        if ch != "\\":
            out.append(ch)
            i += 1
            continue

        i += 1
        if i >= n:
            out.append("\\")
            break

        e = s[i]
        i += 1

        if e == "n":
            out.append("\n")
        elif e == "r":
            out.append("\r")
        elif e == "t":
            out.append("\t")
        elif e == "a":
            out.append("\a")
        elif e == "b":
            out.append("\b")
        elif e == "f":
            out.append("\f")
        elif e == "v":
            out.append("\v")
        elif e in ["\\", '"', "'", "?"]:
            out.append(e)
        elif e == "x":
            hex_digits = []
            while i < n and s[i] in "0123456789abcdefABCDEF":
                hex_digits.append(s[i])
                i += 1
            if not hex_digits:
                out.append("x")
            else:
                out.append(chr(int("".join(hex_digits), 16)))
        elif e.isdigit():
            oct_digits = [e]
            for _ in range(2):
                if i < n and s[i] in "01234567":
                    oct_digits.append(s[i])
                    i += 1
                else:
                    break
            out.append(chr(int("".join(oct_digits), 8)))
        else:
            out.append(e)

    return "".join(out)


@dataclass(frozen=True)
class Entry:
    text: str
    size: int
    index: int


def latin1_bytes(s: str) -> bytes:
    try:
        return s.encode("latin-1")
    except UnicodeEncodeError as e:
        raise RuntimeError(f"Non-latin1 char encountered in '{s}': {e}") from e


def parse_entries_from_txt(src: str, label: str) -> List[Entry]:
    entries: List[Entry] = []
    for lineno, line in enumerate(src.splitlines(), start=1):
        if not line.strip():
            continue
        m = ENTRY_RE.match(line)
        if not m:
            snippet = line.rstrip("\n")
            raise RuntimeError(f"{label}: line {lineno} did not match entry pattern: {snippet}")

        raw = m.group("raw")
        size = int(m.group("size"))
        index = int(m.group("index"))
        text = c_unescape(raw)
        entries.append(Entry(text=text, size=size, index=index))

    return entries


def write_map(path: str, entries: List[Entry]) -> None:
    declared_count = len(entries)
    max_len = 0
    encoded: List[PyTuple[bytes, int, int]] = []

    for entry in entries:
        text = latin1_bytes(entry.text)
        max_len = max(max_len, len(text))
        encoded.append((text, entry.size, entry.index))

    with open(path, "wb") as f:
        f.write(struct.pack("<II", declared_count, max_len))
        for text, size, index in encoded:
            f.write(struct.pack("<ii", size, index))
            f.write(text)
            f.write(b"\x00" * (max_len - len(text)))

    print(f"Wrote {path}: count={declared_count}, max_len={max_len}, rec_size={8 + max_len}")


INDEX_BITS = 18
INDEX_MASK = (1 << INDEX_BITS) - 1
DIRECT = 1 << 31
NO_ROUTE = (1 << 32) - 1


def entry_key(entry: Entry) -> bytes:
    text = latin1_bytes(entry.text)
    if entry.size < 0 or entry.size > len(text):
        raise RuntimeError(
            f"Invalid comparison size {entry.size} for {text!r} ({len(text)} bytes)"
        )
    return text[: entry.size]


def write_ranks(path: str, entries: List[Entry], map_entries: List[Entry]) -> None:
    if len(entries) != len(map_entries):
        raise RuntimeError(
            f"Rank/map count mismatch: {len(entries)} != {len(map_entries)}"
        )

    with open(path, "wb") as f:
        for rank, entry in enumerate(entries):
            if entry.index < 0 or entry.index > INDEX_MASK:
                raise RuntimeError(f"Rank {rank} has invalid map index {entry.index}")
            if entry.size >= (1 << (32 - INDEX_BITS)):
                raise RuntimeError(f"Rank {rank} has oversized comparison length {entry.size}")

            mapped = map_entries[entry.index]
            if mapped.index != entry.index or mapped.text != entry.text:
                raise RuntimeError(
                    f"Rank {rank} does not reference the same map text at {entry.index}"
                )
            entry_key(entry)
            f.write(struct.pack("<I", (entry.size << INDEX_BITS) | entry.index))

    print(f"Wrote {path}: count={len(entries)}, rec_size=4")


def write_lookup(path: str, prefixes: List[Entry], ranks: List[Entry]) -> None:
    routes = [NO_ROUTE] * 256
    spans: List[PyTuple[int, int, int]] = []

    for prefix in prefixes:
        text = latin1_bytes(prefix.text)
        if len(text) != 1:
            raise RuntimeError(f"Prefix must contain one byte: {text!r}")
        byte = text[0]
        if routes[byte] != NO_ROUTE:
            raise RuntimeError(f"Duplicate prefix route for byte 0x{byte:02x}")
        if prefix.index < 0 or prefix.index >= len(ranks):
            raise RuntimeError(f"Prefix rank is out of range: {prefix.index}")

        if prefix.size == 1:
            routes[byte] = DIRECT | ranks[prefix.index].index
            continue

        end = prefix.index + prefix.size
        if prefix.size < 1 or end > len(ranks):
            raise RuntimeError(
                f"Invalid rank range [{prefix.index}, {end}) for byte 0x{byte:02x}"
            )

        first_span = len(spans)
        start = prefix.index
        previous = entry_key(ranks[start])
        for rank in range(start + 1, end):
            current = entry_key(ranks[rank])
            if previous < current:
                add_span(spans, ranks, start, rank)
                start = rank
            previous = current
        add_span(spans, ranks, start, end)

        span_count = len(spans) - first_span
        if first_span >= (1 << 16) or span_count >= (1 << 15):
            raise RuntimeError("Lookup span route exceeds its packed representation")
        routes[byte] = first_span | (span_count << 16)

    with open(path, "wb") as f:
        f.write(struct.pack("<4sII", b"JSCI", len(routes), len(spans)))
        f.write(struct.pack(f"<{len(routes)}I", *routes))
        for start, count, sizes in spans:
            f.write(struct.pack("<III", start, count, sizes))

    print(f"Wrote {path}: routes={len(routes)}, spans={len(spans)}")


def add_span(
    spans: List[PyTuple[int, int, int]], ranks: List[Entry], start: int, end: int
) -> None:
    sizes = 0
    previous = None
    for entry in ranks[start:end]:
        key = entry_key(entry)
        if previous is not None and previous < key:
            raise RuntimeError(f"Rank span [{start}, {end}) is not descending")
        if entry.size >= 32:
            raise RuntimeError(f"Comparison length {entry.size} does not fit the size mask")
        sizes |= 1 << entry.size
        previous = key
    spans.append((start, end - start, sizes))


def read_text_file(path: str) -> str:
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        return f.read()


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--map", required=True, help="Path to jsc_map.txt")
    ap.add_argument("--list", required=True, help="Path to jsc_list.txt")
    ap.add_argument("--prefix", required=True, help="Path to jsc_prefix.txt")
    ap.add_argument("--outdir", required=True, help="Output directory for .bin files")
    args = ap.parse_args()

    os.makedirs(args.outdir, exist_ok=True)

    map_src = read_text_file(args.map)
    list_src = read_text_file(args.list)
    prefix_src = read_text_file(args.prefix)

    map_entries = parse_entries_from_txt(map_src, os.path.basename(args.map))
    list_entries = parse_entries_from_txt(list_src, os.path.basename(args.list))
    prefix_entries = parse_entries_from_txt(prefix_src, os.path.basename(args.prefix))

    write_map(os.path.join(args.outdir, "jsc_map.bin"), map_entries)
    write_ranks(os.path.join(args.outdir, "jsc_list.bin"), list_entries, map_entries)
    write_lookup(os.path.join(args.outdir, "jsc_prefix.bin"), prefix_entries, list_entries)


if __name__ == "__main__":
    main()
