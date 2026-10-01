#!/usr/bin/env python3
"""Exercise the Core release workflow with isolated Docker command fixtures."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().with_name("release-business-core.sh")
OLD = "sha256:" + "a" * 64
NEW = "sha256:" + "b" * 64
DOCKER = r"""#!/usr/bin/env python3
import os, pathlib, sys
root = pathlib.Path(os.environ["RELEASE_TEST_ROOT"])
args = sys.argv[1:]
with (root / "calls").open("a") as log:
    log.write(" ".join(args) + "\n")
old = "sha256:" + "a" * 64
new = "sha256:" + "b" * 64
mode = os.environ["RELEASE_TEST_MODE"]
if args[0] == "inspect":
    template = args[-1]
    if "working_dir" in template:
        print(root)
    elif "config_files" in template:
        print(root / "compose.yml")
    elif "com.docker.compose.project" in template:
        print("fixture")
    elif "com.docker.compose.service" in template:
        print("business-core")
    elif "Running" in template:
        print("true")
    else:
        print((root / "image").read_text())
elif args[:2] == ["image", "inspect"]:
    print(new)
elif args[0] == "compose":
    if "run" in args:
        if "--no-build" in args:
            print("unknown flag: --no-build", file=sys.stderr)
            sys.exit(1)
        if mode == "preflight_failure":
            sys.exit(1)
        pending = 1 if mode == "pending" else 0
        print(f"business-migration-preflight: compatible; database head=76, release head=76, pending={pending}")
    elif "up" in args:
        override = pathlib.Path(args[args.index("up") - 1])
        image = old if override.name == "rollback.yml" else new
        (root / "image").write_text(image)
        if mode == "startup_failure" and image == new:
            sys.exit(1)
elif args[0] == "exec":
    current = (root / "image").read_text()
    if mode == "health_failure" and current == new:
        sys.exit(1)
"""


class CoreReleaseTest(unittest.TestCase):
    def run_release(self, mode, dry_run=False):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "compose.yml").write_text("services: {}\n")
            (root / "image").write_text(OLD)
            binaries = root / "bin"
            binaries.mkdir()
            for name, contents in {
                "docker": DOCKER,
                "flock": "#!/bin/sh\nexit 0\n",
                "sleep": "#!/bin/sh\nexit 0\n",
            }.items():
                command = binaries / name
                command.write_text(contents)
                command.chmod(0o755)
            env = dict(os.environ)
            env.update(
                PATH=str(binaries) + os.pathsep + env["PATH"],
                RELEASE_TEST_ROOT=str(root),
                RELEASE_TEST_MODE=mode,
            )
            command = [
                "bash", str(SCRIPT), "--image", "candidate",
                "--container", "core", "--release-root", str(root / "releases"),
            ]
            if dry_run:
                command.append("--dry-run")
            result = subprocess.run(command, env=env, capture_output=True, text=True)
            return result, (root / "image").read_text(), (root / "calls").read_text()

    def test_success_pins_candidate_and_preserves_dependencies(self):
        result, image, calls = self.run_release("success")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(image, NEW)
        self.assertIn("--no-deps --no-build --pull never business-core", calls)

    def test_dry_run_leaves_running_image_unchanged(self):
        result, image, calls = self.run_release("success", dry_run=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(image, OLD)
        self.assertNotIn(" up ", calls)

    def test_preflight_failure_or_pending_migrations_never_switch(self):
        for mode in ["preflight_failure", "pending"]:
            with self.subTest(mode=mode):
                result, image, calls = self.run_release(mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(image, OLD)
                self.assertNotIn(" up ", calls)

    def test_startup_and_health_failures_restore_previous_image(self):
        for mode in ["startup_failure", "health_failure"]:
            with self.subTest(mode=mode):
                result, image, calls = self.run_release(mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(image, OLD)
                self.assertIn("rollback.yml up", calls)
                self.assertIn("rollback healthy", result.stderr)


if __name__ == "__main__":
    unittest.main()
