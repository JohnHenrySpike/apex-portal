#!/usr/bin/env python3
"""Translation files of Apex 5 Control (see src/lang.rs and locales/README.md).

    tools/i18n.py check                     every locale against the code
    tools/i18n.py update CODE [--language NAME]
                                            make or refresh locales/CODE.json:
                                            new texts empty, gone texts removed,
                                            in the order the code has them
"""

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src"
LOCALES = ROOT / "locales"

LIT = r'"(?:[^"\\]|\\.)*"'
CALL = re.compile(r"(?<![\w])(t|key|tr!)\(\s*(" + LIT + ")", re.S)
PLACE = re.compile(r"\{\{|\}\}|\{([^{}]*)\}")
ESCAPES = {"n": "\n", "t": "\t", "r": "\r", "\\": "\\", '"': '"', "'": "'", "0": "\0"}


def unescape(lit: str) -> str:
    """The value of a Rust string literal."""
    s, out, i = lit[1:-1], [], 0
    while i < len(s):
        if s[i] != "\\":
            out.append(s[i])
            i += 1
            continue
        n = s[i + 1]
        if n == "\n":  # a line continuation drops the next line's indent
            i += 2
            while i < len(s) and s[i] in " \t\r\n":
                i += 1
        elif n == "u":
            end = s.index("}", i)
            out.append(chr(int(s[i + 3 : end], 16)))
            i = end + 1
        else:
            out.append(ESCAPES[n])
            i += 2
    return "".join(out)


def english(key: str) -> str:
    return key.split("#", 1)[0]


def places(text: str) -> list:
    return sorted(m.group(1) for m in PLACE.finditer(text) if m.group(1) is not None)


def call_args(text: str, start: int) -> list:
    """The top-level arguments of the call whose '(' ends just before start."""
    args, cur, depth, i = [], "", 1, start
    while True:
        c = text[i]
        m = re.match(LIT, text[i:], re.S) if c == '"' else None
        if m:
            cur += m.group(0)
            i += len(m.group(0))
            continue
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
            if depth == 0:
                break
        if c == "," and depth == 1:
            args.append(cur.strip())
            cur = ""
        else:
            cur += c
        i += 1
    if cur.strip():
        args.append(cur.strip())
    return args


def scan():
    """Keys in the order the code has them, where each is, and errors."""
    keys, where, errors = [], {}, []
    for path in sorted(SRC.rglob("*.rs")):
        if path.name == "lang.rs" and path.parent == SRC:
            continue
        text = path.read_text()
        for m in CALL.finditer(text):
            key = unescape(m.group(2))
            line = text.count("\n", 0, m.start()) + 1
            spot = f"{path.relative_to(ROOT)}:{line}"
            if key not in where:
                keys.append(key)
                where[key] = spot
            if m.group(1) == "tr!":
                args = call_args(text, text.index("(", m.start()) + 1)[1:]
                names = sorted(re.match(r"\s*(\w+)", a).group(1) for a in args)
                if names != sorted(set(places(english(key)))):
                    errors.append(f"{spot}: tr! gives {names} for {key!r}")
    return keys, where, errors


def read(path: Path) -> dict:
    return json.loads(path.read_text())


def write(path: Path, data: dict):
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")


def check() -> int:
    keys, where, errors = scan()
    known = set(keys)
    for path in sorted(LOCALES.glob("*.json")):
        code = path.stem
        try:
            data = read(path)
            strings = data["strings"]
            if not data.get("language"):
                errors.append(f"{code}: no \"language\" name")
        except (ValueError, KeyError) as e:
            errors.append(f"{code}: {e}")
            continue
        for key, text in strings.items():
            if key not in known:
                errors.append(f"{code}: not in the code any more: {key!r}")
            elif text and places(english(key)) != places(text):
                errors.append(f"{code}: places differ: {key!r} → {text!r} ({where[key]})")
        missing = [k for k in keys if not strings.get(k)]
        done = len(keys) - len(missing)
        print(f"{code} ({data.get('language')}): {done}/{len(keys)} translated")
    for e in errors:
        print("error:", e, file=sys.stderr)
    return 1 if errors else 0


def update(code: str, language) -> int:
    if not re.fullmatch(r"[a-z]{2,3}(_[A-Z]{2})?", code) or code == "en":
        print(f"not a language code: {code} (like de, pt_BR)", file=sys.stderr)
        return 1
    keys, _, errors = scan()
    path = LOCALES / f"{code}.json"
    old = read(path) if path.exists() else {"language": "", "strings": {}}
    strings = old["strings"]
    for key in strings:
        if key not in set(keys) and strings[key]:
            print(f"removed: {key!r} → {strings[key]!r}")
    data = {
        "language": language or old.get("language") or code,
        "strings": {k: strings.get(k, "") for k in keys},
    }
    write(path, data)
    missing = sum(1 for v in data["strings"].values() if not v)
    print(f"{path.relative_to(ROOT)}: {len(keys)} texts, {missing} to translate")
    for e in errors:
        print("error:", e, file=sys.stderr)
    return 1 if errors else 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("check")
    u = sub.add_parser("update")
    u.add_argument("code")
    u.add_argument("--language", help="the language's own name, e.g. Deutsch")
    a = p.parse_args()
    return check() if a.cmd == "check" else update(a.code, a.language)


if __name__ == "__main__":
    sys.exit(main())
