#!/usr/bin/env python3
"""Exercise release staging for Windows and Unix without native compilation."""
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
COMMON = [
    "buzz-acp", "buzz-agent", "buzz-dev-mcp", "life-workbench-mcp",
    "business-read-mcp", "git-credential-nostr", "buzz",
]

for target in ("x86_64-pc-windows-msvc", "aarch64-apple-darwin", "x86_64-unknown-linux-gnu"):
    suffix = ".exe" if "windows" in target else ""
    names = COMMON + ([] if suffix else ["buzz-backend-kubernetes"])
    with tempfile.TemporaryDirectory(prefix="business-sidecars-") as directory:
        root = Path(directory)
        source = root / "target" / target / "release"
        source.mkdir(parents=True)
        for name in names:
            if name != "business-read-mcp":
                (source / (name + suffix)).write_bytes(name.encode())
        command = ["bash", str(ROOT / "scripts/bundle-sidecars.sh"), target]
        missing = subprocess.run(command, cwd=root, capture_output=True, text=True)
        assert missing.returncode != 0 and "business-read-mcp" in missing.stderr
        assert not (root / "desktop/src-tauri/binaries").exists()
        payload = b"business MCP fixture for " + target.encode()
        (source / ("business-read-mcp" + suffix)).write_bytes(payload)
        subprocess.run(command, cwd=root, check=True, capture_output=True)
        staged = root / "desktop/src-tauri/binaries" / ("business-read-mcp-" + target + suffix)
        assert staged.read_bytes() == payload
        if not suffix:
            assert staged.stat().st_mode & 0o111
    print(f"{target}: missing MCP rejected; correct binary staged")

for name in ("tauri.conf.json", "tauri.windows.conf.json"):
    config = json.loads((ROOT / "desktop/src-tauri" / name).read_text())
    assert "binaries/business-read-mcp" in config["bundle"]["externalBin"]
print("Tauri bundle configurations include Business MCP")
