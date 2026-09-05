#!/usr/bin/env python3
"""Record the final exported IPA executable for sideload verification."""
import hashlib
from pathlib import Path, PurePosixPath
import plistlib
import sys
import zipfile


def record_payload(ipa: Path, output: Path) -> None:
    with zipfile.ZipFile(ipa) as archive:
        plists = [name for name in archive.namelist()
                  if len(PurePosixPath(name).parts) == 3
                  and name.startswith('Payload/') and name.endswith('.app/Info.plist')]
        if len(plists) != 1:
            raise ValueError('Expected one top-level app in IPA')
        info = plistlib.loads(archive.read(plists[0]))
        executable = info['CFBundleExecutable']
        if not isinstance(executable, str) or '/' in executable or '\\' in executable:
            raise ValueError('Invalid bundle executable name')
        digest = hashlib.sha256()
        with archive.open(str(PurePosixPath(plists[0]).parent / executable)) as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b''):
                digest.update(chunk)
    output.write_text(f'{digest.hexdigest()}  {executable}\n', encoding='utf-8')


if __name__ == '__main__':
    record_payload(Path(sys.argv[1]), Path(sys.argv[2]))
