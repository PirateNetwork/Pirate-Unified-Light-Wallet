from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import zipfile


SCRIPT_PATH = Path(__file__).parents[1] / "prefetch-komodo-assets.py"
SPEC = importlib.util.spec_from_file_location("prefetch_komodo_assets", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class PrefetchKomodoAssetsTest(unittest.TestCase):
    def _fixture(
        self,
    ) -> tuple[tempfile.TemporaryDirectory[str], Path, Path, Path, Path]:
        temporary = tempfile.TemporaryDirectory()
        root = Path(temporary.name)
        package_root = root / "sdk"
        config_path = package_root / "app_build" / "build_config.json"
        config_path.parent.mkdir(parents=True)
        commit = "c" * 40
        mapped_files = {
            "assets/config/coins_config.json": "utils/coins_config_unfiltered.json",
            "assets/config/coins.json": "coins",
            "assets/config/seed_nodes.json": "seed-nodes.json",
        }
        mapped_folders = {"assets/coin_icons/png/": "icons"}
        config_path.write_text(
            json.dumps(
                {
                    "coins": {
                        "bundled_coins_repo_commit": commit,
                        "mapped_files": mapped_files,
                        "mapped_folders": mapped_folders,
                    },
                },
            ),
            encoding="utf-8",
        )
        (package_root / "assets" / "config").mkdir(parents=True)
        (package_root / "assets" / "config" / ".gitkeep").touch()
        (package_root / "assets" / "coin_icons" / "png").mkdir(parents=True)
        (package_root / "assets" / "coin_icons" / "png" / ".gitkeep").touch()

        package_config = root / "app" / ".dart_tool" / "package_config.json"
        package_config.parent.mkdir(parents=True)
        package_config.write_text(
            json.dumps(
                {
                    "configVersion": 2,
                    "packages": [
                        {
                            "name": "komodo_defi_framework",
                            "rootUri": "../../sdk/",
                            "packageUri": "lib/",
                        },
                    ],
                },
            ),
            encoding="utf-8",
        )

        archive = root / "coins.zip"
        archive_root = f"coins-{commit}"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
            output.writestr(
                f"{archive_root}/utils/coins_config_unfiltered.json",
                '{"coins": []}\n',
            )
            output.writestr(f"{archive_root}/coins", "[]\n")
            output.writestr(f"{archive_root}/seed-nodes.json", "{}\n")
            output.writestr(f"{archive_root}/icons/ARRR.png", b"arrr-icon")
            output.writestr(f"{archive_root}/icons/nested/BTC.png", b"btc-icon")

        lock = root / "asset-lock.json"
        lock.write_text(
            json.dumps(
                {
                    "repository": "kmdclassic/coins",
                    "commit": commit,
                    "archive_url": (
                        "https://codeload.github.com/kmdclassic/coins/zip/" + commit
                    ),
                    "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
                },
            ),
            encoding="utf-8",
        )
        return temporary, package_config, package_root, archive, lock

    def test_materializes_and_then_hash_validates_the_pinned_snapshot(self) -> None:
        temporary, package_config, package_root, archive, lock = self._fixture()
        self.addCleanup(temporary.cleanup)

        resolved, mapped_files, folder_files, cached = MODULE.prepare(
            package_config,
            lock,
            archive_override=archive,
        )

        self.assertEqual(resolved, package_root)
        self.assertEqual((mapped_files, folder_files, cached), (3, 2, False))
        self.assertEqual(
            (package_root / "assets/config/coins_config.json").read_text(),
            '{"coins": []}\n',
        )
        self.assertEqual(
            (package_root / "assets/coin_icons/png/ARRR.png").read_bytes(),
            b"arrr-icon",
        )
        self.assertTrue(
            (package_root / "assets/coin_icons/png/nested/BTC.png").is_file(),
        )
        self.assertTrue((package_root / "assets/config/.gitkeep").is_file())
        self.assertTrue((package_root / "assets/coin_icons/png/.gitkeep").is_file())

        stale = package_root / "assets/coin_icons/png/stale.png"
        stale.write_bytes(b"not-in-the-pinned-snapshot")
        self.assertEqual(
            MODULE.prepare(package_config, lock, archive_override=archive)[1:],
            (3, 2, False),
        )
        self.assertFalse(stale.exists())

        archive.unlink()
        self.assertEqual(
            MODULE.prepare(package_config, lock, archive_override=archive)[1:],
            (3, 2, True),
        )

    def test_rejects_a_lock_that_does_not_match_the_sdk_commit(self) -> None:
        temporary, package_config, package_root, archive, lock = self._fixture()
        self.addCleanup(temporary.cleanup)
        lock_value = json.loads(lock.read_text(encoding="utf-8"))
        lock_value["commit"] = "d" * 40
        lock_value["archive_url"] = (
            "https://codeload.github.com/kmdclassic/coins/zip/" + "d" * 40
        )
        lock.write_text(json.dumps(lock_value), encoding="utf-8")

        with self.assertRaisesRegex(MODULE.AssetPreparationError, "does not match"):
            MODULE.prepare(package_config, lock, archive_override=archive)

        self.assertFalse((package_root / "assets/config/coins.json").exists())

    def test_rejects_an_archive_with_the_wrong_checksum(self) -> None:
        temporary, package_config, package_root, archive, lock = self._fixture()
        self.addCleanup(temporary.cleanup)
        archive.write_bytes(archive.read_bytes() + b"tampered")

        with self.assertRaisesRegex(MODULE.AssetPreparationError, "checksum mismatch"):
            MODULE.prepare(package_config, lock, archive_override=archive)

        self.assertFalse((package_root / "assets/config/coins.json").exists())

    def test_rejects_archive_path_traversal(self) -> None:
        temporary, package_config, package_root, archive, lock = self._fixture()
        self.addCleanup(temporary.cleanup)
        with zipfile.ZipFile(archive, "a") as output:
            output.writestr("../escape", "forbidden")
        lock_value = json.loads(lock.read_text(encoding="utf-8"))
        lock_value["archive_sha256"] = hashlib.sha256(archive.read_bytes()).hexdigest()
        lock.write_text(json.dumps(lock_value), encoding="utf-8")

        with self.assertRaisesRegex(MODULE.AssetPreparationError, "Unsafe.*path"):
            MODULE.prepare(package_config, lock, archive_override=archive)

        self.assertFalse((package_root.parent / "escape").exists())


if __name__ == "__main__":
    unittest.main()
