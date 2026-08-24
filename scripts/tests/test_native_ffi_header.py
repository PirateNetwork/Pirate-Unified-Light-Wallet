from pathlib import Path
import unittest

from scripts import verify_native_ffi_header


PROJECT_ROOT = Path(__file__).parents[2]
RUST_SOURCE = PROJECT_ROOT / "crates" / "pirate-ffi-native" / "src" / "lib.rs"
HEADER = PROJECT_ROOT / "crates" / "pirate-ffi-native" / "pirate_wallet_service.h"
BUILD_SCRIPT = PROJECT_ROOT / "scripts" / "build-native-ffi.sh"


class NativeFfiHeaderTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.rust_source = RUST_SOURCE.read_text(encoding="utf-8")
        cls.header = HEADER.read_text(encoding="utf-8")

    def test_checked_in_header_matches_reviewed_rust_abi(self) -> None:
        self.assertEqual(
            verify_native_ffi_header.verify(self.rust_source, self.header),
            [],
        )

    def test_missing_header_export_is_rejected(self) -> None:
        altered = self.header.replace(
            "void pirate_wallet_service_free_string(char *ptr);",
            "",
        )
        errors = verify_native_ffi_header.verify(self.rust_source, altered)
        self.assertTrue(any("Header exports differ" in error for error in errors))

    def test_rust_signature_drift_is_rejected(self) -> None:
        altered = self.rust_source.replace("pretty: bool", "pretty: u8", 1)
        errors = verify_native_ffi_header.verify(altered, self.header)
        self.assertIn(
            "Rust signature drifted for pirate_wallet_service_invoke_json",
            errors,
        )

    def test_packaging_never_runs_opportunistic_codegen(self) -> None:
        build_script = BUILD_SCRIPT.read_text(encoding="utf-8")
        self.assertIn("cargo build --release --locked", build_script)
        self.assertIn("verify_native_ffi_header.py", build_script)
        self.assertLess(
            build_script.index("verify_native_ffi_header.py"),
            build_script.index("cargo build --release --locked"),
        )
        self.assertNotIn("command -v cbindgen", build_script)
        self.assertNotRegex(build_script, r"(?m)^\s*cbindgen\s")

if __name__ == "__main__":
    unittest.main()
