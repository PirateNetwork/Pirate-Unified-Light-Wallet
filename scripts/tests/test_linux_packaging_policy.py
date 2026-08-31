from pathlib import Path
import re
import unittest


PROJECT_ROOT = Path(__file__).parents[2]
FETCH_ASSETS = PROJECT_ROOT / "scripts" / "fetch-tor-i2p-assets.sh"
LINUX_BUILD = PROJECT_ROOT / "scripts" / "build-linux.sh"
CI_WORKFLOW = PROJECT_ROOT / ".github" / "workflows" / "ci.yml"


class LinuxPackagingPolicyTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.fetch_assets = FETCH_ASSETS.read_text(encoding="utf-8")
        cls.linux_build = LINUX_BUILD.read_text(encoding="utf-8")
        cls.workflow = CI_WORKFLOW.read_text(encoding="utf-8")

    def test_linux_i2pd_is_built_from_an_immutable_source_revision(self) -> None:
        self.assertRegex(
            self.fetch_assets,
            r'I2PD_LINUX_SOURCE="\$\{I2PD_LINUX_SOURCE:-only\}"',
        )
        self.assertRegex(
            self.fetch_assets,
            r'I2PD_COMMIT="\$\{I2PD_COMMIT:-[0-9a-f]{40}\}"',
        )
        self.assertRegex(
            self.fetch_assets,
            r"make \\\s+USE_STATIC=yes",
        )
        self.assertIn("USE_UPNP=no", self.fetch_assets)
        self.assertIn("-static-libstdc++", self.fetch_assets)
        self.assertIn('"$I2P_DIR/linux/$dest_name" --version', self.fetch_assets)

    def test_release_job_installs_i2pd_source_dependencies(self) -> None:
        match = re.search(
            r"(?ms)^  package-linux:\n(?P<body>.*?)(?=^  [\w-]+:\n|\Z)",
            self.workflow,
        )
        self.assertIsNotNone(match)
        package_linux = match.group("body")
        for package in (
            "build-essential",
            "libboost-program-options-dev",
            "libssl-dev",
            "zlib1g-dev",
        ):
            self.assertIn(package, package_linux)

    def test_published_linux_binaries_use_the_ubuntu_2204_floor(self) -> None:
        self.assertRegex(
            self.workflow,
            r"platform: Linux\n\s+os: ubuntu-22\.04\n",
        )
        self.assertRegex(
            self.workflow,
            r"platform: Linux aarch64\n\s+os: ubuntu-22\.04-arm\n",
        )
        self.assertRegex(
            self.workflow,
            r"(?s)package-native-ffi:.*?runs-on: ubuntu-22\.04",
        )
        self.assertRegex(
            self.workflow,
            r"(?s)package-linux:.*?runs-on: ubuntu-22\.04",
        )

    def test_release_bundle_has_a_glibc_235_gate(self) -> None:
        self.assertRegex(
            self.linux_build,
            r'verify_linux_glibc\.py" \\\s+--max-version 2\.35',
        )
        self.assertIn("Verify Linux CLI compatibility", self.workflow)
        self.assertIn("Verify Linux Qortal JNI compatibility", self.workflow)
        self.assertIn("Verify Linux native FFI compatibility", self.workflow)

    def test_appimage_embeds_the_pinned_static_runtime(self) -> None:
        self.assertIn(
            'APPIMAGETOOL_URL: "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage"',
            self.workflow,
        )
        self.assertIn(
            'APPIMAGETOOL_SHA256: "ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"',
            self.workflow,
        )
        self.assertIn(
            'APPIMAGE_RUNTIME_URL: "https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64"',
            self.workflow,
        )
        self.assertIn(
            'APPIMAGE_RUNTIME_SHA256: "2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d"',
            self.workflow,
        )
        self.assertNotIn("obsolete-appimagetool", self.workflow)
        self.assertIn('--runtime-file "$appimage_runtime"', self.linux_build)
        self.assertIn("verify_static_appimage_runtime", self.linux_build)
        self.assertRegex(
            self.linux_build,
            r'python3 "\$SCRIPT_DIR/verify_appimage_runtime\.py" \\\s+'
            r'"\$OUTPUT_DIR/Stashi-Wallet-linux-x86_64\.AppImage" \\\s+'
            r'"\$appimage_runtime"',
        )
        self.assertNotIn("cmp --silent --bytes", self.linux_build)
        self.assertIn("grep -aFq 'libfuse.so.2'", self.linux_build)


if __name__ == "__main__":
    unittest.main()
