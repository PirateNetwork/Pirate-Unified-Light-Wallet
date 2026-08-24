from pathlib import Path
import unittest


PROJECT_ROOT = Path(__file__).parents[2]
GENERATOR = PROJECT_ROOT / "scripts" / "generate-native-ffi-header.sh"
CBINDGEN_CONFIG = PROJECT_ROOT / "crates" / "pirate-ffi-native" / "cbindgen.toml"


class NativeFfiHeaderGenerationTest(unittest.TestCase):
    def test_header_generation_is_explicit_and_stable_compatible(self) -> None:
        generator = GENERATOR.read_text(encoding="utf-8")
        config = CBINDGEN_CONFIG.read_text(encoding="utf-8")
        self.assertIn('CBINDGEN_VERSION="${CBINDGEN_VERSION:-0.29.3}"', generator)
        self.assertIn("--check", generator)
        self.assertIn('--lockfile "$PROJECT_ROOT/crates/Cargo.lock"', generator)
        self.assertNotIn("expand", config)


if __name__ == "__main__":
    unittest.main()
