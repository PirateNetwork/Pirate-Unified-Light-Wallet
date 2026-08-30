from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


PROJECT_ROOT = Path(__file__).parents[2]
PUBLIC_KEY = PROJECT_ROOT / "release-signing" / "public_key.asc"
EMBEDDED_PUBLIC_KEY = PROJECT_ROOT / "app" / "assets" / "security" / "public_key.asc"
METADATA_README = PROJECT_ROOT / "release-signing" / "README"
COLLECTOR = PROJECT_ROOT / "scripts" / "collect-github-release-assets.sh"
SIGNATURE_BUNDLER = PROJECT_ROOT / "scripts" / "create-release-signature-bundle.sh"
CI_WORKFLOW = PROJECT_ROOT / ".github" / "workflows" / "ci.yml"
MACOS_NOTARIZATION_WORKFLOW = (
    PROJECT_ROOT / ".github" / "workflows" / "complete-macos-notarization.yml"
)
EXPECTED_PRIMARY_FINGERPRINT = "E4FB2399AECCF9B9447DED472CE65343401553A6"
EXPECTED_IDENTITY = "Pirate Unified Wallet"
EXPECTED_EMAIL = "dev@piratechainfoundation.com"


class ReleaseSigningPolicyTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.collector = COLLECTOR.read_text(encoding="utf-8")
        cls.bundler = SIGNATURE_BUNDLER.read_text(encoding="utf-8")
        cls.workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        cls.macos_notarization_workflow = MACOS_NOTARIZATION_WORKFLOW.read_text(
            encoding="utf-8"
        )

    def test_public_key_is_the_unified_wallet_release_key(self) -> None:
        gpg = shutil.which("gpg")
        if gpg is None:
            self.skipTest("gpg is required to inspect the release public key")

        with tempfile.TemporaryDirectory() as home:
            result = subprocess.run(
                [
                    gpg,
                    "--homedir",
                    home,
                    "--batch",
                    "--with-colons",
                    "--show-keys",
                    "--fingerprint",
                    str(PUBLIC_KEY),
                ],
                check=True,
                capture_output=True,
                text=True,
            )

        records = [line.split(":") for line in result.stdout.splitlines()]
        fingerprints = [record[9] for record in records if record[0] == "fpr"]
        identities = [record[9] for record in records if record[0] == "uid"]
        self.assertIn(EXPECTED_PRIMARY_FINGERPRINT, fingerprints)
        self.assertTrue(any(EXPECTED_IDENTITY in identity for identity in identities))
        self.assertTrue(any(EXPECTED_EMAIL in identity for identity in identities))

    def test_app_embeds_the_same_public_key_published_with_releases(self) -> None:
        self.assertEqual(PUBLIC_KEY.read_bytes(), EMBEDDED_PUBLIC_KEY.read_bytes())

    def test_metadata_bundle_includes_instructions_and_public_key(self) -> None:
        self.assertTrue(METADATA_README.is_file())
        self.assertIn(
            'cp -f "$RELEASE_METADATA_README" "$META_DIR/README"',
            self.collector,
        )
        self.assertIn(
            'cp -f "$RELEASE_PUBLIC_KEY" "$META_DIR/public-keys/"',
            self.collector,
        )
        self.assertIn('SHA256SUMS_FILE="$META_DIR/SHA256SUMS"', self.collector)

    def test_signature_bundle_matches_treasure_chest_conventions(self) -> None:
        self.assertIn(
            'CHECKSUM_MANIFEST="$STAGE_DIR/sha256sum-${RELEASE_TAG}.txt"',
            self.bundler,
        )
        self.assertIn('cp -f "$README_SOURCE" "$STAGE_DIR/README"', self.bundler)
        self.assertIn(
            'cp -f "$PUBLIC_KEY_SOURCE" "$STAGE_DIR/public_key.asc"',
            self.bundler,
        )
        self.assertIn("--digest-algo SHA512", self.bundler)
        self.assertIn("--detach-sign", self.bundler)
        self.assertNotIn("--armor", self.bundler)
        self.assertIn(
            'sign_file "$file" "$STAGE_DIR/$filename.sig"',
            self.bundler,
        )
        self.assertIn(
            'sign_file "$CHECKSUM_MANIFEST" "$CHECKSUM_MANIFEST.sig"',
            self.bundler,
        )
        self.assertIn(
            'cp -f "$verification_file" "$RELEASE_DIR/$(basename "$verification_file")"',
            self.bundler,
        )
        self.assertIn("! -name '*.sig'", self.bundler)

    def test_ci_requires_the_unified_wallet_private_key_and_builds_one_bundle(self) -> None:
        self.assertIn(
            f'expected_primary="{EXPECTED_PRIMARY_FINGERPRINT}"',
            self.workflow,
        )
        self.assertIn(
            "gpg --batch --import release-signing/public_key.asc",
            self.workflow,
        )
        self.assertIn("scripts/create-release-signature-bundle.sh", self.workflow)
        self.assertIn(
            '"release/signatures-${GITHUB_REF_NAME}.zip"',
            self.workflow,
        )

    def test_macos_refresh_regenerates_verification_material(self) -> None:
        self.assertIn(
            'cp -f release-signing/README "$meta_dir/README"',
            self.macos_notarization_workflow,
        )
        self.assertIn(
            "release-signing/public_key.asc",
            self.macos_notarization_workflow,
        )
        self.assertIn(
            "scripts/create-release-signature-bundle.sh",
            self.macos_notarization_workflow,
        )
        self.assertIn(
            '"$release_dir/signatures-${RELEASE_TAG}.zip"',
            self.macos_notarization_workflow,
        )


if __name__ == "__main__":
    unittest.main()
