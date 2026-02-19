#!/usr/bin/env python3
"""
VirusTotal scanning helper for CI.

Notes:
- The public VirusTotal API is rate-limited (commonly 4 requests/minute) and has
  strict terms of use; treat uploads as public unless you have a private-scanning
  license.
- This script uploads selected release artifacts, polls for analysis completion
  (best-effort), and writes a JSON + Markdown report with GUI links.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any


VT_API_BASE = "https://www.virustotal.com/api/v3"
VT_GUI_FILE_BASE = "https://www.virustotal.com/gui/file"


SCAN_SUFFIXES = {
    ".apk",
    ".aab",
    ".appimage",
    ".deb",
    ".dmg",
    ".exe",
    ".flatpak",
    ".ipa",
    ".zip",
}


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while True:
            chunk = f.read(1024 * 1024)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


@dataclass
class RateLimiter:
    min_interval_seconds: float
    _next_time: float = 0.0

    def wait(self) -> None:
        now = time.time()
        if now < self._next_time:
            time.sleep(self._next_time - now)
        self._next_time = time.time() + self.min_interval_seconds


class VtError(RuntimeError):
    pass


def run_curl_json(
    *,
    url: str,
    api_key: str,
    rate_limiter: RateLimiter,
    method: str = "GET",
    form_file: Path | None = None,
    max_time_seconds: int = 1800,
) -> dict[str, Any]:
    rate_limiter.wait()
    cmd: list[str] = [
        "curl",
        "-sS",
        "--fail",
        "--max-time",
        str(max_time_seconds),
        "-X",
        method,
        "-H",
        f"x-apikey: {api_key}",
    ]

    if form_file is not None:
        cmd += ["-F", f"file=@{str(form_file)}"]

    cmd.append(url)

    try:
        res = subprocess.run(cmd, check=False, capture_output=True, text=True)
    except FileNotFoundError as e:
        raise VtError(f"curl not found: {e}") from e

    if res.returncode != 0:
        msg = res.stderr.strip() or res.stdout.strip() or "unknown error"
        raise VtError(f"curl failed (rc={res.returncode}) for {url}: {msg}")

    try:
        return json.loads(res.stdout)
    except json.JSONDecodeError as e:
        raise VtError(f"Invalid JSON from {url}: {e}: {res.stdout[:2000]}") from e


def get_upload_url(*, api_key: str, rate_limiter: RateLimiter) -> str:
    data = run_curl_json(
        url=f"{VT_API_BASE}/files/upload_url",
        api_key=api_key,
        rate_limiter=rate_limiter,
        method="GET",
    )
    upload_url = data.get("data")
    if not isinstance(upload_url, str) or not upload_url:
        raise VtError(f"Unexpected upload_url response: {data}")
    return upload_url


def upload_file(*, path: Path, api_key: str, rate_limiter: RateLimiter) -> str:
    # Public API "files" endpoint supports up to 32MB; larger files require an
    # upload URL (up to ~650MB) obtained from /files/upload_url.
    size = path.stat().st_size
    if size > 650 * 1024 * 1024:
        raise VtError(
            f"File too large for public API upload_url flow (>{650}MiB): {path} ({size} bytes)"
        )

    if size > 32 * 1024 * 1024:
        url = get_upload_url(api_key=api_key, rate_limiter=rate_limiter)
    else:
        url = f"{VT_API_BASE}/files"

    resp = run_curl_json(
        url=url,
        api_key=api_key,
        rate_limiter=rate_limiter,
        method="POST",
        form_file=path,
    )

    analysis_id = (
        resp.get("data", {}).get("id") if isinstance(resp.get("data"), dict) else None
    )
    if not isinstance(analysis_id, str) or not analysis_id:
        raise VtError(f"Unexpected upload response: {resp}")
    return analysis_id


def get_analysis(*, analysis_id: str, api_key: str, rate_limiter: RateLimiter) -> dict[str, Any]:
    return run_curl_json(
        url=f"{VT_API_BASE}/analyses/{analysis_id}",
        api_key=api_key,
        rate_limiter=rate_limiter,
        method="GET",
    )


def collect_files(root: Path) -> list[Path]:
    out: list[Path] = []
    for p in root.rglob("*"):
        if not p.is_file():
            continue
        # Normalize suffix checks so ".AppImage" matches.
        suf = p.suffix.lower()
        if suf in SCAN_SUFFIXES:
            out.append(p)
            continue
        if p.name.lower().endswith(".appimage"):
            out.append(p)

    files = sorted(set(out))

    # Prefer signed artifacts when both signed + unsigned variants exist.
    # This keeps API usage down and reflects what users will actually download.
    names = {p.name for p in files}
    filtered: list[Path] = []
    for p in files:
        if "-unsigned" in p.name:
            signed_name = p.name.replace("-unsigned", "")
            if signed_name in names:
                continue
        filtered.append(p)
    return filtered


def write_reports(out_dir: Path, records: list[dict[str, Any]]) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)

    (out_dir / "virustotal-results.json").write_text(
        json.dumps(records, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )

    lines: list[str] = []
    lines.append("# VirusTotal Report")
    lines.append("")
    lines.append(
        "This is a best-effort scan report. Uploading to VirusTotal may make files available to third parties."
    )
    lines.append("")
    lines.append("| File | SHA-256 | Status | Malicious | Suspicious | Link |")
    lines.append("|---|---|---:|---:|---:|---|")

    for r in records:
        name = r.get("name", "")
        sha = r.get("sha256", "")
        status = r.get("analysis_status") or r.get("upload_status") or "unknown"
        stats = r.get("analysis_stats") or {}
        mal = stats.get("malicious")
        susp = stats.get("suspicious")
        mal_s = str(mal) if isinstance(mal, int) else ""
        susp_s = str(susp) if isinstance(susp, int) else ""
        link = r.get("vt_file_url", "")
        lines.append(f"| `{name}` | `{sha}` | {status} | {mal_s} | {susp_s} | {link} |")

    (out_dir / "virustotal-results.md").write_text(
        "\n".join(lines) + "\n", encoding="utf-8"
    )


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("input_dir", type=Path, help="Directory containing build artifacts")
    ap.add_argument("output_dir", type=Path, help="Directory to write reports to")
    ap.add_argument(
        "--min-interval-seconds",
        type=float,
        default=float(os.environ.get("VT_MIN_INTERVAL_SECONDS", "16")),
        help="Minimum delay between API requests (default: 16s for public API friendliness).",
    )
    ap.add_argument(
        "--poll-timeout-seconds",
        type=int,
        default=int(os.environ.get("VT_POLL_TIMEOUT_SECONDS", "1200")),
        help="Max time to poll for analysis completion (default: 1200s).",
    )
    args = ap.parse_args()

    api_key = os.environ.get("VIRUSTOTAL_API_KEY", "").strip()
    if not api_key:
        print("VIRUSTOTAL_API_KEY not set; skipping VirusTotal scanning.", file=sys.stderr)
        return 0

    root = args.input_dir
    files = collect_files(root)
    if not files:
        print(f"No files found to scan under {root}", file=sys.stderr)
        write_reports(args.output_dir, [])
        return 0

    limiter = RateLimiter(min_interval_seconds=args.min_interval_seconds)

    records: list[dict[str, Any]] = []
    for p in files:
        rec: dict[str, Any] = {
            "path": str(p),
            "name": p.name,
            "size_bytes": p.stat().st_size,
        }
        try:
            sha = sha256_file(p)
            rec["sha256"] = sha
            rec["vt_file_url"] = f"{VT_GUI_FILE_BASE}/{sha}"
        except Exception as e:
            rec["upload_status"] = "hash_failed"
            rec["error"] = f"hash: {e}"
            records.append(rec)
            continue

        try:
            analysis_id = upload_file(path=p, api_key=api_key, rate_limiter=limiter)
            rec["analysis_id"] = analysis_id
            rec["upload_status"] = "uploaded"
            rec["analysis_status"] = "queued"
        except Exception as e:
            rec["upload_status"] = "upload_failed"
            rec["error"] = f"upload: {e}"
        records.append(rec)

    # Poll best-effort until completion or timeout.
    deadline = time.time() + args.poll_timeout_seconds
    pending = [r for r in records if r.get("analysis_id") and r.get("upload_status") == "uploaded"]
    while pending and time.time() < deadline:
        still_pending: list[dict[str, Any]] = []
        for r in pending:
            analysis_id = r.get("analysis_id")
            if not isinstance(analysis_id, str) or not analysis_id:
                continue
            try:
                analysis = get_analysis(
                    analysis_id=analysis_id, api_key=api_key, rate_limiter=limiter
                )
                attrs = analysis.get("data", {}).get("attributes", {})
                status = attrs.get("status")
                if isinstance(status, str):
                    r["analysis_status"] = status
                if status == "completed":
                    stats = attrs.get("stats")
                    if isinstance(stats, dict):
                        r["analysis_stats"] = stats
                else:
                    still_pending.append(r)
            except Exception as e:
                # Don't fail the whole job; record and move on.
                r["analysis_status"] = "poll_failed"
                r["error"] = f"poll: {e}"
        pending = still_pending

    for r in pending:
        if r.get("analysis_status") not in ("completed", "poll_failed"):
            r["analysis_status"] = "timeout"

    write_reports(args.output_dir, records)

    # Convenience: surface a concise summary in logs.
    completed = sum(1 for r in records if r.get("analysis_status") == "completed")
    failed = sum(1 for r in records if r.get("upload_status") == "upload_failed")
    timeouts = sum(1 for r in records if r.get("analysis_status") == "timeout")
    print(
        f"VirusTotal scan summary: total={len(records)} completed={completed} upload_failed={failed} timeout={timeouts}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
