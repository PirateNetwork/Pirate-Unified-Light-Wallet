from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


PROJECT_ROOT = Path(__file__).parents[2]
PUBLIC_KEY = (
    PROJECT_ROOT
    / "release-signing"
    / "pirate-unified-wallet-release-public-key.asc"
)
METADATA_README = PROJECT_ROOT / "release-signing" / "README.md"
COLLECTOR = PROJECT_ROOT / "scripts" / "collect-github-release-assets.sh"
CI_WORKFLOW = PROJECT_ROOT / ".github" / "workflows" / "ci.yml"
MACOS_NOTARIZATION_WORKFLOW = (
    PROJECT_ROOT / ".github" / "workflows" / "complete-macos-notarization.yml"
)
EXPECTED_FINGERPRINT = "E4FB2399AECCF9B9447DED472CE65343401553A6"
CURRENT_EMAIL = "dev@piratechainfoundation.com"
REVOKED_EMAIL = "dev@pirate.black"


class ReleaseSigningPolicyTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.collector = COLLECTOR.read_text(encoding="utf-8")
        cls.workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        cls.macos_notarization_workflow = MACOS_NOTARIZATION_WORKFLOW.read_text(
            encoding="utf-8"
        )

    def test_public_key_has_the_expected_fingerprint_and_identity(self) -> None:
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
                    str(PUBLIC_KEY),
                ],
                check=True,
                capture_output=True,
                text=True,
            )

        records = [line.split(":") for line in result.stdout.splitlines()]
        fingerprints = [record[9] for record in records if record[0] == "fpr"]
        self.assertIn(EXPECTED_FINGERPRINT, fingerprints)

        current_uids = [
            record for record in records if record[0] == "uid" and CURRENT_EMAIL in record[9]
        ]
        revoked_uids = [
            record for record in records if record[0] == "uid" and REVOKED_EMAIL in record[9]
        ]
        self.assertTrue(current_uids)
        self.assertTrue(all(record[1] != "r" for record in current_uids))
        self.assertTrue(revoked_uids)
        self.assertTrue(all(record[1] == "r" for record in revoked_uids))

    def test_metadata_bundle_includes_instructions_and_public_key(self) -> None:
        self.assertTrue(METADATA_README.is_file())
        self.assertIn(
            'cp -f "$RELEASE_METADATA_README" "$META_DIR/README.md"',
            self.collector,
        )
        self.assertIn(
            'cp -f "$RELEASE_PUBLIC_KEY" "$META_DIR/public-keys/"',
            self.collector,
        )
        self.assertIn('SHA256SUMS_FILE="$META_DIR/SHA256SUMS"', self.collector)

    def test_ci_pins_the_expected_release_key(self) -> None:
        self.assertIn(
            f'expected_fingerprint="{EXPECTED_FINGERPRINT}"',
            self.workflow,
        )
        self.assertIn(
            "gpg --batch --import release-signing/"
            "pirate-unified-wallet-release-public-key.asc",
            self.workflow,
        )
        self.assertIn('--local-user "$GPG_SIGNING_KEY"', self.workflow)
        self.assertIn('path: dist/linux-signatures/*.asc', self.workflow)
        self.assertIn("-name '*.flatpak'", self.workflow)

    def test_macos_metadata_refresh_preserves_verification_material(self) -> None:
        self.assertIn(
            'cp -f release-signing/README.md "$meta_dir/README.md"',
            self.macos_notarization_workflow,
        )
        self.assertIn(
            "release-signing/pirate-unified-wallet-release-public-key.asc",
            self.macos_notarization_workflow,
        )
        self.assertIn(
            'sort -k2 "$meta_dir/SHA256SUMS.tmp" > "$meta_dir/SHA256SUMS"',
            self.macos_notarization_workflow,
        )


if __name__ == "__main__":
    unittest.main()
