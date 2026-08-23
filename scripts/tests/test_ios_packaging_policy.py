from pathlib import Path
import re
import unittest


PROJECT_ROOT = Path(__file__).parents[2]
IOS_BUILD_SCRIPT = PROJECT_ROOT / "scripts" / "build-ios-sdk.sh"
REACT_NATIVE_PACKAGE_SCRIPT = (
    PROJECT_ROOT / "scripts" / "package-react-native-plugin.sh"
)


def exported_value(script: str, variable: str) -> str:
    match = re.search(
        rf"^export {re.escape(variable)}=(?P<value>[^\s#]+)$",
        script,
        re.MULTILINE,
    )
    if match is None:
        raise AssertionError(f"Missing explicit {variable} export")
    return match.group("value")


class IosPackagingPolicyTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.ios_build = IOS_BUILD_SCRIPT.read_text(encoding="utf-8")
        cls.react_native_package = REACT_NATIVE_PACKAGE_SCRIPT.read_text(
            encoding="utf-8",
        )

    def test_static_archives_keep_granular_release_codegen(self) -> None:
        self.assertEqual(
            exported_value(self.ios_build, "CARGO_PROFILE_RELEASE_LTO"),
            "false",
        )
        self.assertEqual(
            exported_value(
                self.ios_build,
                "CARGO_PROFILE_RELEASE_CODEGEN_UNITS",
            ),
            "16",
        )
        self.assertEqual(
            exported_value(self.ios_build, "CARGO_PROFILE_RELEASE_DEBUG"),
            "0",
        )
        self.assertEqual(
            exported_value(self.ios_build, "CARGO_PROFILE_RELEASE_STRIP"),
            "debuginfo",
        )

    def test_each_publishable_ios_archive_has_a_size_gate(self) -> None:
        self.assertIn(
            'IOS_NPM_MAX_COMPRESSED_ARCHIVE_BYTES="${IOS_NPM_MAX_COMPRESSED_ARCHIVE_BYTES:-190000000}"',
            self.ios_build,
        )
        self.assertRegex(
            self.ios_build,
            r'verify_compressed_archive_budget \\\n+  "\$CRATES_DIR/target/aarch64-apple-ios/release/libpirate_ffi_native\.a" \\\n+  "ios-arm64"',
        )
        self.assertRegex(
            self.ios_build,
            r'verify_compressed_archive_budget \\\n+  "\$CRATES_DIR/target/aarch64-apple-ios-sim/release/libpirate_ffi_native\.a" \\\n+  "ios-simulator-arm64"',
        )
        self.assertRegex(
            self.ios_build,
            r'verify_compressed_archive_budget \\\n+  "\$CRATES_DIR/target/x86_64-apple-ios/release/libpirate_ffi_native\.a" \\\n+  "ios-simulator-x86_64"',
        )
        self.assertNotRegex(
            self.ios_build,
            r'verify_compressed_archive_budget \\\n+  "\$SIM_LIB"',
        )

    def test_simulator_npm_payloads_are_architecture_split(self) -> None:
        self.assertIn(
            '"react-native-pirate-wallet-ios-simulator-arm64"',
            self.react_native_package,
        )
        self.assertIn(
            '"react-native-pirate-wallet-ios-simulator-x86_64"',
            self.react_native_package,
        )
        self.assertNotIn(
            '"react-native-pirate-wallet-ios-simulator"',
            self.react_native_package,
        )

    def test_temporary_swift_package_tree_is_not_uploaded(self) -> None:
        archive_index = self.ios_build.index(
            'ditto -c -k --sequesterRsrc --keepParent PirateWalletSDK-package',
        )
        cleanup_index = self.ios_build.rindex('rm -rf "$PACKAGE_STAGING"')
        self.assertGreater(cleanup_index, archive_index)

    def test_oversized_npm_packages_report_largest_files(self) -> None:
        self.assertIn(
            'Largest packed files:',
            self.react_native_package,
        )
        self.assertIn(
            '$package_name-npm-pack.json',
            self.react_native_package,
        )

    def test_packaging_emits_wrapper_and_five_native_companions(self) -> None:
        package_entries = re.findall(
            r'^  "react-native-pirate-wallet[^\"]*"$',
            self.react_native_package,
            re.MULTILINE,
        )
        self.assertEqual(len(package_entries), 6)


if __name__ == "__main__":
    unittest.main()
