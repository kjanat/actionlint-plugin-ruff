import hashlib
import tempfile
import unittest
from pathlib import Path

import release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.package = self.directory / "artifacts"
        self.package.mkdir()
        for name in release.FILES:
            (self.package / name).write_bytes(name.encode())
        self.checksums = "".join(
            f"{release.digest(self.package / name)}  {name}\n" for name in release.FILES
        )
        (self.package / "SHA256SUMS").write_text(self.checksums, encoding="utf-8")

    def test_tag_matches_crate_version(self):
        manifest = self.directory / "Cargo.toml"
        manifest.write_text('[package]\nversion = "0.1.0"\n', encoding="utf-8")
        release.check_tag("v0.1.0", manifest, manifest)
        for tag in ("master", "v0.2.0", "0.1.0"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                release.check_tag(tag, manifest, manifest)

    def test_tag_rejects_different_component_version(self):
        manifest = self.directory / "Cargo.toml"
        component = self.directory / "component.toml"
        manifest.write_text('[package]\nversion = "0.1.0"\n', encoding="utf-8")
        component.write_text('[package]\nversion = "0.2.0"\n', encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "versions must match"):
            release.check_tag("v0.1.0", manifest, component)

    def test_verifies_all_packaged_files_and_tested_digest(self):
        checksum = hashlib.sha256(b"plugin.component.wasm").hexdigest()
        self.assertEqual(release.verify(self.package, checksum), checksum)
        with self.assertRaisesRegex(ValueError, "tested component"):
            release.verify(self.package, "0" * 64)
        (self.package / "schema.json").write_text("changed", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "schema.json"):
            release.verify(self.package, checksum)

    def test_rejects_missing_extra_duplicate_and_traversal_checksums(self):
        for checksums in (
            self.checksums.splitlines()[0] + "\n",
            self.checksums + self.checksums.splitlines()[0] + "\n",
            self.checksums.replace("plugin.wasm", "../plugin.wasm"),
        ):
            with self.subTest(checksums=checksums):
                (self.package / "SHA256SUMS").write_text(checksums, encoding="utf-8")
                with self.assertRaises(ValueError):
                    release.verify(self.package)
        (self.package / "unexpected").write_bytes(b"")
        with self.assertRaisesRegex(ValueError, "exactly"):
            release.verify(self.package)


if __name__ == "__main__":
    unittest.main()
