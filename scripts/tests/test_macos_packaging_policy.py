from pathlib import Path
import re
import unittest


PROJECT_ROOT = Path(__file__).parents[2]
MACOS_BUILD = PROJECT_ROOT / "scripts" / "build-macos.sh"
MACOS_DMG_VERIFY = PROJECT_ROOT / "scripts" / "verify-macos-dmg.sh"
MACOS_ENTITLEMENTS_VERIFY = PROJECT_ROOT / "scripts" / "verify-macos-entitlements.sh"
MACOS_ENTITLEMENTS = (
    PROJECT_ROOT / "app" / "macos" / "Runner" / "DebugProfile.entitlements",
    PROJECT_ROOT / "app" / "macos" / "Runner" / "Release.entitlements",
    PROJECT_ROOT / "app" / "macos" / "Runner" / "Distribution.entitlements",
)


class MacosPackagingPolicyTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.macos_build = MACOS_BUILD.read_text(encoding="utf-8")
        cls.macos_dmg_verify = MACOS_DMG_VERIFY.read_text(encoding="utf-8")

    def test_adhoc_resigning_remains_portable(self) -> None:
        function = re.search(
            r"(?ms)^adhoc_sign_app_bundle\(\) \{(?P<body>.*?)^\}",
            self.macos_build,
        )
        self.assertIsNotNone(function)
        body = function.group("body")

        self.assertNotIn("entitlements_path", body)
        self.assertNotIn("--entitlements", body)
        self.assertIn('codesign --force --sign "$identity" "$app_path"', body)
        self.assertIn(
            'adhoc_sign_app_bundle "$APP_PATH"',
            self.macos_build,
        )

    def test_portable_packages_do_not_require_keychain_access_groups(self) -> None:
        self.assertFalse(MACOS_ENTITLEMENTS_VERIFY.exists())
        self.assertNotIn("verify-macos-entitlements.sh", self.macos_build)
        self.assertNotIn("verify-macos-entitlements.sh", self.macos_dmg_verify)

        for path in MACOS_ENTITLEMENTS:
            with self.subTest(path=path):
                contents = path.read_text(encoding="utf-8")
                self.assertNotIn("keychain-access-groups", contents)


if __name__ == "__main__":
    unittest.main()
