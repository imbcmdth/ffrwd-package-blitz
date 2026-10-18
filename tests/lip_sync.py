"""Checks the output of tests/lip_sync.sql: three feeds, each with the
flashes at 1, 2 and 3 s of its segment, every flash's beep on the same
instant, sound minus picture within 1 ms. Needs ffmpeg and ffprobe on the
PATH.

usage: python tests/lip_sync.py lip_sync.mkv
"""
import array
import subprocess
import sys

RATE = 48000


def run(args):
    return subprocess.run(args, capture_output=True, check=True).stdout


def flashes(path):
    """The time of each frame that turns white after one that was not."""
    times = [
        float(line.split(b",")[0])
        for line in run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
                         "frame=pts_time", "-of", "csv=p=0", path]).split()
        if line.split(b",")[0] not in (b"", b"N/A")
    ]
    raw = run(["ffmpeg", "-v", "error", "-i", path, "-map", "0:v:0", "-fps_mode", "passthrough",
               "-vf", "scale=8:8,format=gray", "-f", "rawvideo", "-"])
    bright = [sum(raw[i:i + 64]) / 64 > 128 for i in range(0, len(raw), 64)]
    return [times[i] for i in range(1, min(len(times), len(bright))) if bright[i] and not bright[i - 1]]


def beeps(path):
    """The time of each sample above 0.05 after 0.3 s below it."""
    start = run(["ffprobe", "-v", "error", "-select_streams", "a:0", "-show_entries",
                 "stream=start_time", "-of", "csv=p=0", path]).split()
    start = float(start[0].split(b",")[0]) if start and start[0] != b"N/A" else 0.0
    samples = array.array("f", run(["ffmpeg", "-v", "error", "-i", path, "-map", "0:a:0",
                                    "-ac", "1", "-ar", str(RATE), "-f", "f32le", "-"]))
    out, last = [], -RATE
    for i, s in enumerate(samples):
        if abs(s) > 0.05:
            if i - last > 0.3 * RATE:
                out.append(start + i / RATE)
            last = i
    return out


def main():
    path = sys.argv[1]
    seen, heard = flashes(path), beeps(path)
    feeds, last = [], None
    for f in seen:
        nearest = min(heard, key=lambda b: abs(b - f), default=None)
        if nearest is None or abs(nearest - f) > 0.5:
            sys.exit(f"FAIL: the flash at {f:.3f} s has no beep")
        if last is None or f - last > 1.5:
            feeds.append([])
        feeds[-1].append((f, (nearest - f) * 1000))
        last = f
    for k, feed in enumerate(feeds):
        said = ", ".join(f"{f:.3f} s {d:+.2f} ms" for f, d in feed)
        print(f"feed {k + 1}: {said}")
    if [len(feed) for feed in feeds] != [3, 3, 3]:
        sys.exit(f"FAIL: three feeds of three flashes expected, got {[len(f) for f in feeds]}")
    worst = max(abs(d) for feed in feeds for _, d in feed)
    if worst > 1.0:
        sys.exit(f"FAIL: sound minus picture reaches {worst:.2f} ms")
    print(f"ok: 9 flashes on 3 feeds, sound minus picture within {worst:.2f} ms")


if __name__ == "__main__":
    main()
