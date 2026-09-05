import hashlib
import importlib.util
import json
import re
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("rust_sbom", ROOT / "scripts/extract-rust-sbom.py")
sbom = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sbom)


class RustSbomTest(unittest.TestCase):
    def test_each_packaging_job_extracts_before_cleanup_and_caches_pinned_tools(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        for platform in sbom.LIBRARIES:
            with self.subTest(platform=platform):
                job = re.search(rf"(?ms)^  package-{platform}:\n.*?(?=^  [\w-]+:\n|\Z)", workflow).group()
                self.assertLess(job.index("name: Generate SBOM"), job.index("name: Reclaim workspace disk"))
                self.assertIn(f"./scripts/generate-sbom.sh dist/sbom {platform}", job)
                self.assertIn(".tools/rust-sbom", job)
                self.assertIn("auditable-0.7.2-audit-info-0.5.4-syft-1.40.1", job)

    def test_all_platforms_extract_every_exact_library_and_bind_its_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for platform, libraries in sbom.LIBRARIES.items():
                calls = []
                for target, relative in libraries:
                    path = root / "crates/target" / relative
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(target.encode())

                def run(command, **kwargs):
                    calls.append(command)
                    return subprocess.CompletedProcess(command, 0, json.dumps({
                        "packages": [{"name": "pirate-ffi-frb", "root": True}]
                    }))

                result = sbom.extract(root, platform, Path("extractor"), run)
                self.assertEqual(len(calls), len(libraries))
                for artifact, (target, relative) in zip(result["artifacts"], libraries):
                    self.assertEqual(artifact["target"], target)
                    self.assertEqual(artifact["sha256"], hashlib.sha256(target.encode()).hexdigest())
                    self.assertIn(["extractor", str(root / "crates/target" / relative)], calls)

    def test_missing_target_never_uses_an_unrelated_executable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            other = root / "crates/target/release/other.exe"
            other.parent.mkdir(parents=True)
            other.write_bytes(b"other")
            with self.assertRaisesRegex(ValueError, "Missing"):
                sbom.extract(root, "android", Path("extractor"))

    def test_missing_or_wrong_metadata_fails_instead_of_rebuilding(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "crates/target" / sbom.LIBRARIES["linux"][0][1]
            path.parent.mkdir(parents=True)
            path.write_bytes(b"library")
            for data in ({}, {"packages": []}, {"packages": [{"name": "other", "root": True}]}):
                with self.subTest(data=data), self.assertRaises(ValueError):
                    sbom.extract(root, "linux", Path("extractor"),
                                 lambda command, **kwargs: subprocess.CompletedProcess(command, 0, json.dumps(data)))


if __name__ == "__main__":
    unittest.main()
