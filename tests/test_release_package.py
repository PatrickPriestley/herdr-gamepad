"""Exercise the actual CI packaging block and its commit/checksum contract."""
import hashlib
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest


class PackageTests(unittest.TestCase):
    def test_ci_packages_tested_head_for_both_macos_targets(self):
        root = Path(__file__).resolve().parents[1]
        workflow = (root / ".github/workflows/ci.yml").read_text()
        step = workflow.split(
            "      - name: Package the release binary and its checksum\n", 1
        )[1].split("      - uses:", 1)[0]
        run_block = step.split("        run: |\n", 1)[1]
        script = "\n".join(line[10:] for line in run_block.splitlines())
        for target in ("aarch64-apple-darwin", "x86_64-apple-darwin"):
            with self.subTest(target=target), tempfile.TemporaryDirectory() as name:
                checkout = Path(name)
                release = checkout / "target/release"
                release.mkdir(parents=True)
                (release / "herd-gamepad").write_bytes(b"tested binary\n")
                # A stale stamp makes omission of the stamping command observable.
                (release / "herd-gamepad.commit").write_text("stale\n")
                (checkout / "source").write_text("tested source\n")

                def run(argv):
                    return subprocess.run(
                        argv, cwd=checkout, check=True, text=True, capture_output=True
                    ).stdout

                run(["git", "init", "-q"])
                run(["git", "add", "source"])
                run([
                    "git", "-c", "core.hooksPath=/dev/null",
                    "-c", "commit.gpgsign=false", "-c", "user.name=Test",
                    "-c", "user.email=test@example.invalid", "commit", "-qm", "tested",
                ])
                head = run(["git", "rev-parse", "HEAD"])
                run([
                    "/bin/sh", "-eu", "-c",
                    script.replace("${{ matrix.target }}", target),
                ])
                asset = checkout / f"dist/herd-gamepad-{target}.tar.gz"
                with tarfile.open(asset, "r:gz") as archive:
                    self.assertIn("herd-gamepad.commit", archive.getnames())
                    self.assertEqual(
                        archive.extractfile("herd-gamepad.commit").read().decode(), head
                    )
                    self.assertEqual(
                        archive.extractfile("herd-gamepad").read(), b"tested binary\n"
                    )
                self.assertEqual(
                    asset.with_suffix(asset.suffix + ".sha256").read_text().split()[0],
                    hashlib.sha256(asset.read_bytes()).hexdigest(),
                )


if __name__ == "__main__":
    unittest.main()
