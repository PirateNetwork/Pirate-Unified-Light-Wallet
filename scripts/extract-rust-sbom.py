"""Read embedded dependency records from explicit packaging inputs, without Cargo."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess


LIBRARIES = {
    "windows": [("x86_64-pc-windows-msvc", "x86_64-pc-windows-msvc/release/pirate_ffi_frb.dll")],
    "linux": [("host-linux", "release/libpirate_ffi_frb.so")],
    "macos": [
        (target, f"{target}/release/libpirate_ffi_frb.dylib")
        for target in ("aarch64-apple-darwin", "x86_64-apple-darwin")
    ],
    "ios": [("aarch64-apple-ios", "aarch64-apple-ios/release/libpirate_ffi_frb.dylib")],
    "android": [
        (target, f"{target}/release/libpirate_ffi_frb.so")
        for target in ("aarch64-linux-android", "armv7-linux-androideabi", "x86_64-linux-android")
    ],
}


def extract(root, platform, extractor, run=subprocess.run):
    artifacts = []
    for target, relative in LIBRARIES[platform]:
        library = root / "crates/target" / relative
        if not library.is_file():
            raise ValueError(f"Missing {platform} release library: {library}")
        result = run([str(extractor), str(library)], check=True, capture_output=True, text=True)
        audit = json.loads(result.stdout)
        packages = audit.get("packages") if isinstance(audit, dict) else None
        if not isinstance(packages, list) or not packages:
            raise ValueError(f"Missing embedded dependency records: {library}")
        if not any(p.get("name") == "pirate-ffi-frb" and p.get("root") is True for p in packages):
            raise ValueError(f"Dependency records do not identify the wallet library: {library}")
        digest = hashlib.sha256()
        with library.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
        artifacts.append({
            "target": target,
            "path": library.relative_to(root).as_posix(),
            "sha256": digest.hexdigest(),
            "dependencies": audit,
        })
    return {"schemaVersion": 1, "scope": "rust-packaging-inputs", "platform": platform, "artifacts": artifacts}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--platform", choices=LIBRARIES, required=True)
    parser.add_argument("--extractor", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    # Discard stale output before checking every required architecture. Never rebuild
    # or substitute a host executable/source inventory when extraction fails.
    args.output.unlink(missing_ok=True)
    document = extract(args.root.resolve(), args.platform, args.extractor)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
