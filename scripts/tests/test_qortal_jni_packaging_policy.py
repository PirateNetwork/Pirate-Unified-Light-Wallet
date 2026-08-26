from pathlib import Path
import re
import unittest


PROJECT_ROOT = Path(__file__).parents[2]
CI_WORKFLOW = PROJECT_ROOT / ".github" / "workflows" / "ci.yml"


class QortalJniPackagingPolicyTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        match = re.search(
            r"(?ms)^  package-qortal-jni:\n(?P<body>.*?)(?=^  [\w-]+:\n|\Z)",
            workflow,
        )
        if match is None:
            raise AssertionError("Missing package-qortal-jni workflow job")
        cls.job = match.group("body")

    def test_macos_targets_run_on_matching_native_architectures(self) -> None:
        self.assertRegex(
            self.job,
            r"platform: macOS x86_64\n"
            r"\s+os: macos-15-intel\n"
            r"\s+target: x86_64-apple-darwin\n",
        )
        self.assertRegex(
            self.job,
            r"platform: macOS aarch64\n"
            r"\s+os: macos-15\n"
            r"\s+target: aarch64-apple-darwin\n",
        )


if __name__ == "__main__":
    unittest.main()
