"""Checks the output of tests/closed_captions.sql: the closed captions'
rows, in the order they went out, with their times on the programme's
timeline and their casters' voices.

usage: python tests/closed_captions.py closed_captions.ndjson
"""
import json
import sys

# text, start_t, end_t, final, voice. Each cue starts when the display
# shows it, or when the closed captions came on; a final row ends it 3 s
# (`hold`) later, and switching them off ends a cue still growing there.
EXPECTED = [
    ("he goes", 1.0, 1.0, False, "Mango"),
    ("what a read", 1.2, 4.2, True, "Ludwig"),
    ("he goes for the", 1.0, 1.5, False, "Mango"),
    ("he goes for the flank", 1.0, 5.0, True, "Mango"),
    ("it was close", 2.2, 2.2, False, "Ludwig"),
    ("it was close one", 2.2, 5.6, True, "Ludwig"),
    ("and that", 3.0, 3.0, False, "MangoPlays"),
    ("and that", 3.0, 3.5, True, "MangoPlays"),
    ("and that is game", 4.0, 4.0, False, "MangoPlays"),
    ("and that is game five", 4.0, 7.5, True, "MangoPlays"),
]


def main(path):
    with open(path, encoding="utf-8") as f:
        rows = [json.loads(line) for line in f if line.strip()]
    got = [(r["text"], r["start_t"], r["end_t"], r["final"], r.get("voice")) for r in rows]
    for row in got:
        print(row)
    if got != EXPECTED:
        print("FAIL: expected")
        for row in EXPECTED:
            print(row)
        return 1
    print(f"ok: {len(got)} rows")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
