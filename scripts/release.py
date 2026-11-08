"""Check the release tag and the exact packaged file set."""

import hashlib
import os
import re
import sys
import tomllib
from pathlib import Path

FILES = (
    "plugin.wasm",
    "plugin.wasm.gz",
    "metadata.json",
    "schema.json",
    "LICENSE",
    "LICENSE-RUFF",
    "plugin.component.wasm",
    "plugin.component.wasm.gz",
    "component.metadata.json",
)
COMPONENT = "plugin.component.wasm"


def check_tag(
    tag, manifest=Path("Cargo.toml"), component=Path("crates/component/Cargo.toml")
):
    version = tomllib.loads(manifest.read_text(encoding="utf-8"))["package"]["version"]
    component_version = tomllib.loads(component.read_text(encoding="utf-8"))["package"][
        "version"
    ]
    if component_version != version:
        raise ValueError("Core and component package versions must match")
    if tag != "v" + version or not re.fullmatch(
        r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?", tag
    ):
        raise ValueError(f"Release tag must equal v{version}")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify(directory=Path("artifacts"), expected=None):
    names = set(FILES) | {"SHA256SUMS"}
    if {path.name for path in directory.iterdir()} != names:
        raise ValueError("Package must contain exactly the ten release files")
    checksums = {}
    for line in (directory / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_.-]+)", line)
        if match is None or match[2] not in FILES or match[2] in checksums:
            raise ValueError("Malformed package checksums")
        checksums[match[2]] = match[1]
    if set(checksums) != set(FILES):
        raise ValueError("Incomplete package checksums")
    for name, checksum in checksums.items():
        if digest(directory / name) != checksum:
            raise ValueError(f"Package checksum mismatch: {name}")
    if expected is not None and checksums[COMPONENT] != expected:
        raise ValueError("Package differs from the tested component")
    return checksums[COMPONENT]


def main():
    command = sys.argv[1]
    if command == "check-tag":
        check_tag(os.environ["RELEASE_TAG"])
    elif command == "verify":
        checksum = verify()
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write(f"sha256={checksum}\n")
    else:
        raise ValueError("Unknown release command")


if __name__ == "__main__":
    main()
