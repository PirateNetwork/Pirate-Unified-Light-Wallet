from pathlib import Path
import re
import unittest


PROJECT_ROOT = Path(__file__).parents[2]
MACOS_BUILD = PROJECT_ROOT / "scripts" / "build-macos.sh"
MACOS_DMG_VERIFY = PROJECT_ROOT / "scripts" / "verify-macos-dmg.sh"
MACOS_ENTITLEMENTS_VERIFY = (
    PROJECT_ROOT / "scripts" / "verify-macos-entitlements.sh"
)


class MacosPackagingPolicyTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.macos_build = MACOS_BUILD.read_text(encoding="utf-8")
        cls.macos_dmg_verify = MACOS_DMG_VERIFY.read_text(encoding="utf-8")
        cls.macos_entitlements_verify = MACOS_ENTITLEMENTS_VERIFY.read_text(
            encoding="utf-8",
        )

    def test_adhoc_resigning_reapplies_release_entitlements(self) -> None:
        function = re.search(
            r"(?ms)^adhoc_sign_app_bundle\(\) \{(?P<body>.*?)^\}",
            self.macos_build,
        )
        self.assertIsNotNone(function)
        body = function.group("body")

        self.assertIn('local entitlements_path="$2"', body)
        self.assertRegex(
            body,
            r'codesign --force --sign "\$identity" \\\n'
            r'\s+--entitlements "\$entitlements_path" \\\n'
            r'\s+"\$app_path"',
        )
        self.assertIn(
            'adhoc_sign_app_bundle "$APP_PATH" "$ENTITLEMENTS_PATH"',
            self.macos_build,
        )

    def test_built_and_packaged_apps_verify_keychain_entitlement(self) -> None:
        verification_call = (
            'bash "$SCRIPT_DIR/verify-macos-entitlements.sh" "$APP_PATH"'
        )
        self.assertIn(verification_call, self.macos_build)
        self.assertIn(verification_call, self.macos_dmg_verify)
        self.assertIn(
            "-c 'Print :keychain-access-groups'",
            self.macos_entitlements_verify,
        )
        self.assertIn(
            'codesign --display --entitlements - "$APP_PATH"',
            self.macos_entitlements_verify,
        )


if __name__ == "__main__":
    unittest.main()
