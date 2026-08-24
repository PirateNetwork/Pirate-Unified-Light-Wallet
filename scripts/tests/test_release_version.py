import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest


PROJECT_ROOT = Path(__file__).parents[2]
VERSION_SCRIPT = PROJECT_ROOT / "scripts" / "sync-version-from-tag.sh"


class ReleaseVersionTest(unittest.TestCase):
    def run_sync(
        self,
        tag: str | None = None,
        *,
        environment: dict[str, str] | None = None,
    ) -> tuple[subprocess.CompletedProcess[str], str]:
        with tempfile.TemporaryDirectory(
            dir=PROJECT_ROOT / "scripts" / "tests",
        ) as temp_dir:
            pubspec = Path(temp_dir) / "pubspec.yaml"
            pubspec.write_text(
                "name: release_version_fixture\nversion: 0.0.1+1\n",
                encoding="utf-8",
            )
            env = os.environ.copy()
            env.pop("GITHUB_REF", None)
            env.pop("GITHUB_REF_NAME", None)
            env.pop("GITHUB_REF_TYPE", None)
            env.pop("VERSION_BUILD_NUMBER", None)
            env["VERSION_PUBSPEC_PATH"] = str(pubspec)
            if environment:
                env.update(environment)
            if os.name == "nt":
                def wsl_path(path: Path) -> str:
                    drive = path.drive.rstrip(":").lower()
                    return f"/mnt/{drive}{path.as_posix()[2:]}"

                assignments = {
                    "VERSION_PUBSPEC_PATH": wsl_path(pubspec),
                    "GITHUB_REF": env.get("GITHUB_REF", ""),
                    "GITHUB_REF_NAME": env.get("GITHUB_REF_NAME", ""),
                    "GITHUB_REF_TYPE": env.get("GITHUB_REF_TYPE", ""),
                    "VERSION_BUILD_NUMBER": env.get(
                        "VERSION_BUILD_NUMBER",
                        "",
                    ),
                }
                exports = " ".join(
                    f"{key}={shlex.quote(value)}"
                    for key, value in assignments.items()
                )
                invocation = f"bash {shlex.quote(wsl_path(VERSION_SCRIPT))}"
                if tag is not None:
                    invocation += f" {shlex.quote(tag)}"
                command = [
                    "bash",
                    "-c",
                    f"export {exports}; {invocation}",
                ]
            else:
                command = ["bash", str(VERSION_SCRIPT)]
                if tag is not None:
                    command.append(tag)
            result = subprocess.run(
                command,
                cwd=PROJECT_ROOT,
                env=env,
                check=False,
                capture_output=True,
                text=True,
            )
            return result, pubspec.read_text(encoding="utf-8")

    def test_explicit_tag_sets_semver_and_monotonic_build_number(self) -> None:
        result, pubspec = self.run_sync("v1.1.8")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(
            "version: 1.1.8+10108",
            pubspec,
            f"stdout={result.stdout!r} stderr={result.stderr!r}",
        )

    def test_github_tag_environment_is_resolved(self) -> None:
        result, pubspec = self.run_sync(
            environment={
                "GITHUB_REF_TYPE": "tag",
                "GITHUB_REF_NAME": "v2.4.3",
            },
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(
            "version: 2.4.3+20403",
            pubspec,
            f"stdout={result.stdout!r} stderr={result.stderr!r}",
        )

    def test_ci_can_override_the_build_number(self) -> None:
        result, pubspec = self.run_sync(
            "refs/tags/v1.1.8",
            environment={"VERSION_BUILD_NUMBER": "4242"},
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(
            "version: 1.1.8+4242",
            pubspec,
            f"stdout={result.stdout!r} stderr={result.stderr!r}",
        )

    def test_malformed_release_tag_fails_closed(self) -> None:
        result, pubspec = self.run_sync("v1.1")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must match vX.Y.Z", result.stderr)
        self.assertIn("version: 0.0.1+1", pubspec)

    def test_non_tag_build_preserves_the_committed_baseline(self) -> None:
        result, pubspec = self.run_sync()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("version: 0.0.1+1", pubspec)


if __name__ == "__main__":
    unittest.main()
