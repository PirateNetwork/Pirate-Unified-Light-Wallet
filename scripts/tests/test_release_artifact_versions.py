import copy
import importlib.util
from pathlib import Path
import unittest


PROJECT_ROOT = Path(__file__).parents[2]
DETECTOR_PATH = PROJECT_ROOT / "scripts" / "detect-release-artifacts.py"
SPEC = importlib.util.spec_from_file_location("detect_release_artifacts", DETECTOR_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Unable to load {DETECTOR_PATH}")
DETECTOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DETECTOR)


class ReleaseArtifactVersionTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.original_cwd = Path.cwd()
        # The release detector intentionally resolves manifests from repo root.
        import os

        os.chdir(PROJECT_ROOT)

    @classmethod
    def tearDownClass(cls) -> None:
        import os

        os.chdir(cls.original_cwd)

    def manifest(self) -> dict:
        return DETECTOR.load_manifest(None)

    def test_release_manifest_matches_every_versioned_source(self) -> None:
        DETECTOR.validate_source_versions(self.manifest())

    def test_rust_artifact_drift_fails_validation(self) -> None:
        manifest = copy.deepcopy(self.manifest())
        manifest["native_ffi"]["version"] = "999.0.0"

        with self.assertRaisesRegex(ValueError, "pirate-ffi-native"):
            DETECTOR.validate_source_versions(manifest)

    def test_android_sdk_drift_fails_validation(self) -> None:
        manifest = copy.deepcopy(self.manifest())
        manifest["android_sdk"]["version"] = "999.0.0"

        with self.assertRaisesRegex(ValueError, "Android SDK"):
            DETECTOR.validate_source_versions(manifest)

    def test_react_native_drift_fails_validation(self) -> None:
        manifest = copy.deepcopy(self.manifest())
        manifest["react_native_plugin"]["version"] = "999.0.0"

        with self.assertRaisesRegex(ValueError, "React Native package"):
            DETECTOR.validate_source_versions(manifest)


if __name__ == "__main__":
    unittest.main()
