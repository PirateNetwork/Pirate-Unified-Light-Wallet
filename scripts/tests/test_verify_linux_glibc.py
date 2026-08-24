from pathlib import Path
import tempfile
import unittest
from unittest import mock

from scripts import verify_linux_glibc


class VerifyLinuxGlibcTest(unittest.TestCase):
    def test_parser_ignores_private_and_non_glibc_symbols(self) -> None:
        output = "GLIBC_2.17 GLIBC_PRIVATE GLIBCXX_3.4.30 GLIBC_2.35"
        self.assertEqual(
            verify_linux_glibc.parse_glibc_versions(output),
            {"2.17", "2.35"},
        )

    def test_audit_rejects_any_elf_above_the_floor(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            compatible = Path(temp_dir) / "compatible.so"
            incompatible = Path(temp_dir) / "incompatible.so"
            compatible.write_bytes(b"\x7fELFcompatible")
            incompatible.write_bytes(b"\x7fELFincompatible")

            def versions(path: Path, _readelf: str) -> set[str]:
                if path == incompatible:
                    return {"2.2.5", "2.38"}
                return {"2.2.5", "2.35"}

            with mock.patch.object(
                verify_linux_glibc,
                "read_glibc_versions",
                side_effect=versions,
            ):
                inspected, violations = verify_linux_glibc.audit(
                    [Path(temp_dir)],
                    "2.35",
                    "readelf",
                )

        self.assertEqual(len(inspected), 2)
        self.assertEqual(violations, [(incompatible, "2.38")])

    def test_empty_scan_is_a_hard_failure(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            result = verify_linux_glibc.main([temp_dir])
        self.assertEqual(result, 2)


if __name__ == "__main__":
    unittest.main()
