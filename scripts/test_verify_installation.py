import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import verify_installation


class InstallationProofTests(unittest.TestCase):
    def test_contract_includes_packaged_wit_digest_with_lf_normalization(self):
        wit = (
            Path(__file__).resolve().parents[1]
            / "crates"
            / "component"
            / "wit"
            / "script-engine.wit"
        ).read_bytes()
        expected = (
            "kjanat:actionlint/script-engine@0.1.0#sha256="
            "cb0d4c86f1c4e9e8029368b8686f5da542d8e6e542483aa6dbce2510a35eba86"
        )
        self.assertEqual(verify_installation.contract_identity(wit), expected)
        self.assertEqual(
            verify_installation.contract_identity(
                wit.replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
            ),
            expected,
        )

    def test_installation_proof_binds_pin_cache_and_offline_recheck(self):
        data = b"tested component"
        checksum = hashlib.sha256(data).hexdigest()
        wit = (
            Path(__file__).resolve().parents[1]
            / "crates/component/wit/script-engine.wit"
        ).read_bytes()
        for fault in (None, "pin", "cache", "lock"):
            calls = []

            def invoke(
                binary,
                arguments,
                root,
                environment,
                expected=0,
                *,
                calls=calls,
                fault=fault,
            ):
                calls.append((arguments, environment))
                lock = root / ".github/actionlint.plugins.lock.json"
                if arguments[0] == "version":
                    return {"version": "test-host"}
                if arguments[:2] == ["plugin", "install"]:
                    self.assertEqual(arguments[2], verify_installation.REPOSITORY)
                    self.assertEqual(arguments[3:5], ["--version", "v0.1.0"])
                    lock.parent.mkdir()
                    pin = {
                        "name": verify_installation.REPOSITORY,
                        "plugin_id": "actionlint-plugin-ruff",
                        "selector": "v0.1.0",
                        "version": "v0.1.0",
                        "sha256": "0" * 64 if fault == "pin" else checksum,
                        "kind": "component",
                        "contract": verify_installation.contract_identity(wit),
                    }
                    lock.write_text(json.dumps({"plugins": [pin]}), encoding="utf-8")
                    artifact = root / "cache" / checksum
                    artifact.parent.mkdir()
                    artifact.write_bytes(b"different" if fault == "cache" else data)
                    return {}
                self.assertEqual(
                    arguments, ["check", "--json", "--no-shellcheck", "workflow.yml"]
                )
                self.assertEqual(expected, 1)
                if len(calls) == 4:
                    self.assertEqual(environment["HTTPS_PROXY"], "http://127.0.0.1:1")
                    self.assertEqual(environment["NO_PROXY"], "")
                    if fault == "lock":
                        lock.write_text("changed", encoding="utf-8")
                return {
                    "completed": True,
                    "exit_code": 1,
                    "diagnostics": [
                        {
                            "rule": "ruff",
                            "code": "F821",
                            "start": {"line": 7, "column": 20},
                            "end": {"line": 7, "column": 27},
                        }
                    ],
                }

            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as directory:
                binary = Path(directory) / "actionlint"
                binary.write_bytes(b"fixture")
                with patch.object(verify_installation, "run", side_effect=invoke):
                    if fault:
                        with self.assertRaises(ValueError):
                            verify_installation.verify(binary, "v0.1.0", checksum)
                    else:
                        result = verify_installation.verify(binary, "v0.1.0", checksum)
                        self.assertEqual(
                            result["locked_check_with_blocked_proxy"], "passed"
                        )
                        self.assertEqual(len(calls), 4)

    def test_rejects_incomplete_wrong_or_additional_findings(self):
        result = {
            "completed": True,
            "exit_code": 1,
            "diagnostics": [
                {
                    "rule": "ruff",
                    "code": "F821",
                    "start": {"line": 7, "column": 20},
                    "end": {"line": 7, "column": 27},
                }
            ],
        }
        self.assertEqual(
            verify_installation.check_result(result), result["diagnostics"]
        )
        for field, value in (("completed", False), ("exit_code", 3)):
            invalid = copy.deepcopy(result)
            invalid[field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                verify_installation.check_result(invalid)
        for field, value in (
            ("rule", "shellcheck"),
            ("code", "FAKE999"),
            ("start", {"line": 1, "column": 6}),
        ):
            invalid = copy.deepcopy(result)
            invalid["diagnostics"][0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                verify_installation.check_result(invalid)
        for findings in ([], result["diagnostics"] * 2):
            invalid = copy.deepcopy(result)
            invalid["diagnostics"] = findings
            with self.subTest(findings=findings), self.assertRaises(ValueError):
                verify_installation.check_result(invalid)


if __name__ == "__main__":
    unittest.main()
