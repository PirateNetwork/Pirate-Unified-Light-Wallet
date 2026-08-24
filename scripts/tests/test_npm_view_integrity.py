import json
from pathlib import Path
import subprocess
import tempfile
import unittest


PROJECT_ROOT = Path(__file__).parents[2]
SCRIPT = PROJECT_ROOT / "scripts" / "read-npm-view-integrity.js"
WORKFLOW = PROJECT_ROOT / ".github" / "workflows" / "ci.yml"
INTEGRITY = "sha512-ZmFrZS1idXQtdmFsaWQtYmFzZTY0LWRpZ2VzdA=="


class NpmViewIntegrityTest(unittest.TestCase):
    def run_parser(self, value: object) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as temp_dir:
            response = Path(temp_dir) / "npm-view.json"
            response.write_text(json.dumps(value), encoding="utf-8")
            return subprocess.run(
                ["node", str(SCRIPT), str(response)],
                cwd=PROJECT_ROOT,
                check=False,
                capture_output=True,
                text=True,
            )

    def test_accepts_npm_10_scalar_response(self) -> None:
        result = self.run_parser(INTEGRITY)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, INTEGRITY)

    def test_accepts_npm_12_single_result_array(self) -> None:
        result = self.run_parser([INTEGRITY])

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, INTEGRITY)

    def test_rejects_ambiguous_multiple_results(self) -> None:
        result = self.run_parser([INTEGRITY, INTEGRITY])

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Expected exactly one", result.stderr)

    def test_rejects_non_string_result(self) -> None:
        result = self.run_parser({"integrity": INTEGRITY})

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Expected a registry integrity string", result.stderr)

    def test_rejects_malformed_integrity(self) -> None:
        result = self.run_parser("not-an-integrity")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not a valid SHA-512", result.stderr)

    def test_publication_workflow_uses_the_checked_in_parser(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        checkout = workflow.index("- name: Checkout publication verifier")
        verification = workflow.index("- name: Verify npm packages", checkout)

        self.assertLess(checkout, verification)
        self.assertIn(
            'node scripts/read-npm-view-integrity.js "$view_json"',
            workflow[verification:],
        )
        self.assertNotIn(
            'process.stdout.write(JSON.parse(',
            workflow[verification:],
        )


if __name__ == "__main__":
    unittest.main()
