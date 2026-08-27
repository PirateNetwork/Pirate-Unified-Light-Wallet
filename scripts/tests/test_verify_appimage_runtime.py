from pathlib import Path
import struct
import tempfile
import unittest

from scripts import verify_appimage_runtime


RUNTIME_SIZE = 0x300
DIGEST_OFFSET = 0x250


def runtime_fixture() -> bytes:
    runtime = bytearray(RUNTIME_SIZE)
    runtime[:16] = b"\x7fELF\x02\x01\x01\x00AI\x02\x00\x00\x00\x00\x00"
    struct.pack_into("<Q", runtime, 40, 0x100)
    struct.pack_into("<H", runtime, 58, 64)
    struct.pack_into("<H", runtime, 60, 3)
    struct.pack_into("<H", runtime, 62, 1)

    names = b"\x00.shstrtab\x00.digest_md5\x00"
    runtime[0x200 : 0x200 + len(names)] = names

    struct.pack_into("<I", runtime, 0x140, 1)
    struct.pack_into("<I", runtime, 0x140 + 4, 3)
    struct.pack_into("<Q", runtime, 0x140 + 24, 0x200)
    struct.pack_into("<Q", runtime, 0x140 + 32, len(names))

    struct.pack_into("<I", runtime, 0x180, 11)
    struct.pack_into("<I", runtime, 0x180 + 4, 1)
    struct.pack_into("<Q", runtime, 0x180 + 24, DIGEST_OFFSET)
    struct.pack_into("<Q", runtime, 0x180 + 32, 16)
    return bytes(runtime)


class VerifyAppImageRuntimeTest(unittest.TestCase):
    def write_fixture(self, directory: str) -> tuple[Path, Path]:
        runtime = Path(directory) / "runtime-x86_64"
        appimage = Path(directory) / "wallet.AppImage"
        runtime_bytes = runtime_fixture()
        packaged = bytearray(runtime_bytes)
        packaged[DIGEST_OFFSET : DIGEST_OFFSET + 16] = bytes(range(1, 17))
        packaged.extend(b"hsqs" + b"payload")
        runtime.write_bytes(runtime_bytes)
        appimage.write_bytes(packaged)
        return appimage, runtime

    def test_accepts_only_the_appimagetool_digest_rewrite(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            appimage, runtime = self.write_fixture(directory)

            section = verify_appimage_runtime.verify(appimage, runtime)

        self.assertEqual(section.offset, DIGEST_OFFSET)
        self.assertEqual(section.size, 16)

    def test_rejects_a_runtime_change_outside_the_digest_section(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            appimage, runtime = self.write_fixture(directory)
            packaged = bytearray(appimage.read_bytes())
            packaged[0x80] ^= 0xFF
            appimage.write_bytes(packaged)

            with self.assertRaisesRegex(
                verify_appimage_runtime.VerificationError,
                "unauthorized offset 128",
            ):
                verify_appimage_runtime.verify(appimage, runtime)

    def test_rejects_a_payload_at_the_wrong_runtime_boundary(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            appimage, runtime = self.write_fixture(directory)
            packaged = bytearray(appimage.read_bytes())
            packaged[RUNTIME_SIZE : RUNTIME_SIZE + 4] = b"nope"
            appimage.write_bytes(packaged)

            with self.assertRaisesRegex(
                verify_appimage_runtime.VerificationError,
                "SquashFS payload",
            ):
                verify_appimage_runtime.verify(appimage, runtime)

    def test_rejects_an_unfinalized_digest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            appimage, runtime = self.write_fixture(directory)
            packaged = bytearray(appimage.read_bytes())
            packaged[DIGEST_OFFSET : DIGEST_OFFSET + 16] = runtime.read_bytes()[
                DIGEST_OFFSET : DIGEST_OFFSET + 16
            ]
            appimage.write_bytes(packaged)

            with self.assertRaisesRegex(
                verify_appimage_runtime.VerificationError,
                "did not finalize",
            ):
                verify_appimage_runtime.verify(appimage, runtime)


if __name__ == "__main__":
    unittest.main()
