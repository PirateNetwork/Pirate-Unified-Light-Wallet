from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


SCRIPT_PATH = Path(__file__).parents[1] / "configure-komodo-assets.py"
SPEC = importlib.util.spec_from_file_location("configure_komodo_assets", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ConfigureKomodoAssetsTest(unittest.TestCase):
    def _fixture(self) -> tuple[tempfile.TemporaryDirectory[str], Path, Path]:
        temporary = tempfile.TemporaryDirectory()
        root = Path(temporary.name)
        package_root = root / "sdk"
        config_path = package_root / "app_build" / "build_config.json"
        config_path.parent.mkdir(parents=True)

        mapped_files = {
            "assets/config/coins_config.json": "utils/coins_config_unfiltered.json",
            "assets/config/coins.json": "coins",
            "assets/config/seed_nodes.json": "seed-nodes.json",
        }
        for relative in mapped_files:
            asset = package_root.joinpath(*Path(relative).parts)
            asset.parent.mkdir(parents=True, exist_ok=True)
            asset.write_text("{}\n", encoding="utf-8")
        icon = package_root / "assets" / "coin_icons" / "png" / "ARRR.png"
        icon.parent.mkdir(parents=True)
        icon.write_bytes(b"not-empty")

        config = {
            "api": {
                "api_commit_hash": "a" * 40,
                "fetch_at_build_enabled": True,
                "platforms": {"linux": {"valid_zip_sha256_checksums": ["b" * 64]}},
            },
            "coins": {
                "bundled_coins_repo_commit": "c" * 40,
                "fetch_at_build_enabled": True,
                "update_commit_on_build": True,
                "runtime_updates_enabled": True,
                "mapped_files": mapped_files,
                "mapped_folders": {"assets/coin_icons/png/": "icons"},
                "cdn_branch_mirrors": {"main": "https://example.invalid"},
            },
        }
        config_path.write_text(json.dumps(config), encoding="utf-8")

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
        return temporary, package_config, config_path

    def test_disables_network_mutation_without_changing_runtime_policy(self) -> None:
        temporary, package_config, config_path = self._fixture()
        self.addCleanup(temporary.cleanup)

        resolved_path, mapped_files, folder_files, changed = MODULE.configure(
            package_config,
        )

        self.assertEqual(resolved_path, config_path)
        self.assertEqual(mapped_files, 3)
        self.assertEqual(folder_files, 1)
        self.assertTrue(changed)
        config = json.loads(config_path.read_text(encoding="utf-8"))
        self.assertFalse(config["api"]["fetch_at_build_enabled"])
        self.assertFalse(config["coins"]["fetch_at_build_enabled"])
        self.assertFalse(config["coins"]["update_commit_on_build"])
        self.assertTrue(config["coins"]["runtime_updates_enabled"])
        self.assertEqual(
            config["coins"]["cdn_branch_mirrors"],
            {"main": "https://example.invalid"},
        )

        original_bytes = config_path.read_bytes()
        self.assertFalse(MODULE.configure(package_config)[3])
        self.assertEqual(config_path.read_bytes(), original_bytes)

    def test_refuses_to_disable_fetch_when_a_materialized_asset_is_missing(self) -> None:
        temporary, package_config, config_path = self._fixture()
        self.addCleanup(temporary.cleanup)
        missing_asset = config_path.parents[1] / "assets" / "config" / "coins.json"
        missing_asset.unlink()

        with self.assertRaisesRegex(MODULE.ConfigurationError, "missing or empty"):
            MODULE.configure(package_config)

        config = json.loads(config_path.read_text(encoding="utf-8"))
        self.assertTrue(config["api"]["fetch_at_build_enabled"])
        self.assertTrue(config["coins"]["fetch_at_build_enabled"])

    def test_rejects_asset_paths_outside_the_resolved_package(self) -> None:
        temporary, package_config, config_path = self._fixture()
        self.addCleanup(temporary.cleanup)
        config = json.loads(config_path.read_text(encoding="utf-8"))
        config["coins"]["mapped_files"] = {"../secret.json": "secret.json"}
        config_path.write_text(json.dumps(config), encoding="utf-8")

        with self.assertRaisesRegex(MODULE.ConfigurationError, "Unsafe"):
            MODULE.configure(package_config)


if __name__ == "__main__":
    unittest.main()
