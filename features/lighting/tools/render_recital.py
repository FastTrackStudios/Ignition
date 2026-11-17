#!/usr/bin/env python3
"""Render JT's recital light show to video, one song at a time.

    render_recital.py render [slug ...] [--force] [--draft] [--jobs N]
                                                    the lit songs (or these), N at a time
                                                    -> <videos>/<slug>.mp4 (1080p), or
                                                    <videos>/draft/<slug>.mp4 (720p, --draft)
    render_recital.py remux  [slug ...] [--force]   swap in newer audio, keep the picture
    render_recital.py list                          what is rendered, with which audio

Each song is `ignition --set <als> render --song <locator> --audio <audio>`:
frame i is i/30 s after the song's locator, the audio muxed from its first
sample. The audio is the song's real recording when there is one
(<recital>/recordings/<slug>.ogg), else its reference bounce
(<recital>/refs/<slug>.ogg). `<slug>.audio` beside each video names the
audio it carries (path, size, mtime); `remux` re-muxes every video whose
audio has changed since — a stream copy of the picture, seconds per song.

Run inside the dev shell (`nix develop`: the GPU's Vulkan loader), from the
checkout root — `just lighting render-recital` / `just lighting remux-recital`.
Environment: IGNITION_BIN (the built runner), IGNITION_FFMPEG (ffmpeg),
IGNITION_LIVE_SET, RECITAL_DIR (/run/media/Development/fts-dev/recital).
"""

import json
import os
import subprocess
import sys
import time
from pathlib import Path

RECITAL = Path(os.environ.get("RECITAL_DIR", "/run/media/Development/fts-dev/recital"))
VIDEOS = RECITAL / "videos"
LIVE_SET = os.environ.get(
    "IGNITION_LIVE_SET",
    str(RECITAL / "show/Sr. Recital Showcase Project/Sr. Recital Showcase.als"),
)
# The Archive: in the set, not played, not lit.
UNLIT = {"berenstein", "better-strangers"}


def ffmpeg() -> str:
    return os.environ.get("IGNITION_FFMPEG", "ffmpeg")


def songs() -> list[tuple[str, str]]:
    """(slug, locator) of every song of the set, in set order (split.tsv)."""
    out = []
    for line in (RECITAL / "split.tsv").read_text().splitlines():
        cols = line.split("\t")
        if len(cols) >= 2 and cols[0]:
            out.append((cols[0], cols[1]))
    return out


def audio_for(slug: str) -> Path | None:
    for path in (RECITAL / "recordings" / f"{slug}.ogg", RECITAL / "refs" / f"{slug}.ogg"):
        if path.is_file():
            return path
    return None


def stamp(path: Path) -> dict:
    st = path.stat()
    return {"path": str(path), "size": st.st_size, "mtime": int(st.st_mtime)}


def carried(slug: str) -> dict | None:
    sidecar = VIDEOS / f"{slug}.audio"
    try:
        return json.loads(sidecar.read_text())
    except (OSError, ValueError):
        return None


def selected(args: list[str]) -> list[tuple[str, str]]:
    wanted = [a for a in args if not a.startswith("--")]
    rows = [(s, n) for s, n in songs() if s not in UNLIT]
    if wanted:
        known = {s for s, _ in songs()}
        for w in wanted:
            if w not in known:
                sys.exit(f"render_recital: no song {w!r} in split.tsv")
        rows = [(s, n) for s, n in songs() if s in wanted]
    return rows


def duration(path: Path) -> float:
    probe = Path(ffmpeg()).with_name("ffprobe") if os.sep in ffmpeg() else "ffprobe"
    out = subprocess.run(
        [str(probe), "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(path)],
        capture_output=True, text=True, timeout=60, check=True,
    )
    return float(out.stdout.strip())


def option(args: list[str], name: str, default: str) -> str:
    """`--name value` from the arguments."""
    if name in args:
        i = args.index(name)
        if i + 1 < len(args):
            return args[i + 1]
    return default


def render_one(runner: str, slug: str, locator: str, out: Path, audio: Path,
               draft: bool, threads: int) -> tuple[str, bool, str, float]:
    """One song, its output in <out>.log: (slug, ok, its summary line, seconds)."""
    began = time.monotonic()
    cmd = ["nice", "-n", "19", runner, "--set", LIVE_SET, "render",
           "--song", locator, "--out", str(out), "--audio", str(audio),
           # The video covers the whole recording, however it runs past
           # the song's end on the set.
           "--min-length", f"{duration(audio):.6f}"]
    env = dict(os.environ, IGNITION_THREADS=str(threads))
    if draft:
        cmd.append("--draft")
        env.setdefault("IGNITION_QUALITY", "low")
    log = out.with_suffix(".log")
    with open(log, "w") as f:
        ok = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT, env=env,
                            timeout=6 * 3600).returncode == 0
    summary = next((line.strip() for line in reversed(log.read_text().splitlines())
                    if line.startswith("render: ") and " fps " in line), "")
    return slug, ok, summary, time.monotonic() - began


def render(args: list[str]) -> int:
    """`render [slug …] [--force] [--draft] [--jobs N]`: the songs side by side,
    N at a time (the GPU's and the encoder's work overlap; each process gets
    a few worker threads, `IGNITION_THREADS`). `--draft`: 720p at the `low`
    quality into <videos>/draft/ — the night in minutes; without it the
    1080p final into <videos>/."""
    from concurrent.futures import ThreadPoolExecutor

    runner = os.environ.get("IGNITION_BIN", "ignition")
    force = "--force" in args
    draft = "--draft" in args
    jobs = max(1, int(option(args, "--jobs", "8")))
    threads = max(1, (os.cpu_count() or 8) // jobs)
    flags = {"--jobs", option(args, "--jobs", "")}
    names = [a for a in args if a not in flags]
    into = VIDEOS / "draft" if draft else VIDEOS
    into.mkdir(parents=True, exist_ok=True)
    work, failed = [], []
    for slug, locator in selected(names):
        out = into / f"{slug}.mp4"
        audio = audio_for(slug)
        if audio is None:
            print(f"skip {slug}: no recordings/ or refs/ audio")
            failed.append(slug)
        elif out.exists() and not force:
            print(f"have {out} (--force to render again; `remux` for new audio)")
        else:
            work.append((slug, locator, out, audio))
    print(f"rendering {len(work)} songs, {jobs} at a time, {threads} threads each, "
          f"{'720p draft' if draft else '1080p'} into {into}", flush=True)
    began = time.monotonic()
    frames = 0
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        runs = [pool.submit(render_one, runner, slug, loc, out, audio, draft, threads)
                for slug, loc, out, audio in work]
        for run in runs:
            slug, ok, summary, secs = run.result()
            out = into / f"{slug}.mp4"
            if not ok:
                print(f"FAIL {slug} (see {out.with_suffix('.log')})", flush=True)
                failed.append(slug)
                continue
            audio = audio_for(slug)
            (into / f"{slug}.audio").write_text(json.dumps(stamp(audio)) + "\n")
            length = duration(out)
            frames += round(length * 30)
            print(f"ok   {slug:22} {length:8.3f}s  {secs:5.0f}s wall  {summary[8:]}", flush=True)
    wall = time.monotonic() - began
    if work:
        print(f"night: {len(work) - len(failed)} songs, {frames} frames in {wall:.0f}s wall "
              f"= {frames / max(wall, 1e-3):.0f} fps overall", flush=True)
    if failed:
        print("failed:", " ".join(failed))
    return 1 if failed else 0


def remux(args: list[str]) -> int:
    force = "--force" in args
    failed = []
    for slug, _ in selected(args):
        video = VIDEOS / f"{slug}.mp4"
        audio = audio_for(slug)
        if not video.exists() or audio is None:
            continue
        if not force and carried(slug) == stamp(audio):
            continue
        tmp = VIDEOS / f"{slug}.remux.mp4"
        # The picture and its chapters as they are; the new audio from its
        # first sample, cut to the picture's length.
        cmd = [ffmpeg(), "-hide_banner", "-loglevel", "error", "-nostdin", "-y",
               "-i", str(video), "-t", f"{duration(video):.6f}", "-i", str(audio),
               "-map", "0:v", "-map", "1:a", "-map_metadata", "0", "-map_chapters", "0",
               "-c:v", "copy", "-c:a", "aac", "-b:a", "192k",
               "-movflags", "+faststart", "-f", "mp4", str(tmp)]
        if subprocess.run(["nice", "-n", "19", *cmd], timeout=1800).returncode != 0:
            print(f"FAIL {slug}")
            failed.append(slug)
            tmp.unlink(missing_ok=True)
            continue
        tmp.replace(video)
        (VIDEOS / f"{slug}.audio").write_text(json.dumps(stamp(audio)) + "\n")
        print(f"remuxed {slug} with {audio}")
    return 1 if failed else 0


def listing() -> int:
    for slug, _ in songs():
        video = VIDEOS / f"{slug}.mp4"
        audio = audio_for(slug)
        state = "unlit" if slug in UNLIT else ("rendered" if video.exists() else "missing")
        stale = ""
        if video.exists() and audio is not None and carried(slug) != stamp(audio):
            stale = "  (audio changed: remux)"
        length = f"{duration(video):8.3f}s" if video.exists() else " " * 9
        print(f"{slug:22} {state:9} {length} {audio or '-'}{stale}")
    return 0


def main() -> int:
    if len(sys.argv) < 2 or sys.argv[1] not in ("render", "remux", "list"):
        print(__doc__)
        return 2
    verb, rest = sys.argv[1], sys.argv[2:]
    return {"render": render, "remux": remux, "list": lambda _: listing()}[verb](rest)


if __name__ == "__main__":
    sys.exit(main())
