"""Isolated connector regression tests; never modify real client settings."""

import importlib.machinery
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent


def load_installer():
    """Load the extensionless Python installer without running its CLI."""
    loader = importlib.machinery.SourceFileLoader(
        "connectors", str(ROOT / "install-sessionmesh-connectors")
    )
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


class ConnectorTests(unittest.TestCase):
    def test_json_update_preserves_settings_and_original_backup(self):
        module = load_installer()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "settings.json"
            path.write_text('{"model":"keep","hooks":{"Other":[]}}\n')
            original = path.read_bytes()
            module.update_json(path, lambda data: {**data, "mcp": {"sessionmesh": {}}})
            module.update_json(path, lambda data: {**data, "another": True})
            self.assertEqual(json.loads(path.read_text())["model"], "keep")
            self.assertEqual(
                Path(str(path) + ".sessionmesh-backup").read_bytes(), original
            )
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_instruction_append_is_idempotent_and_preserves_existing_text(self):
        module = load_installer()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "AGENTS.md"
            path.write_text("# Existing rules\nKeep this rule.\n")
            module.install_instruction(path)
            first = path.read_bytes()
            module.install_instruction(path)
            self.assertEqual(path.read_bytes(), first)
            self.assertIn(b"Keep this rule.", first)

    def test_client_commands_are_user_scoped_and_match_cli_syntax(self):
        module = load_installer()
        bridge = "/tmp/path with spaces/bridge"
        self.assertEqual(
            module.registration("agy", bridge),
            ["agy", "mcp", "add", "sessionmesh", bridge],
        )
        self.assertEqual(
            module.registration("opencode", bridge),
            ["opencode", "mcp", "add", "--global", "sessionmesh", "--", bridge],
        )
        self.assertIn("user", module.registration("grok", bridge))
        self.assertIn("user", module.registration("claude", bridge))

    def test_dry_run_never_invokes_clients_or_writes(self):
        module = load_installer()
        with patch.object(module.subprocess, "run") as run, patch.object(
            module, "backup"
        ) as backup:
            module.main(["--dry-run", "codex", "claude", "agy", "opencode", "grok"])
            run.assert_not_called()
            backup.assert_not_called()

    def test_hook_update_preserves_other_commands_and_is_idempotent(self):
        module = load_installer()
        original = {
            "model": "keep",
            "hooks": {
                "Other": [],
                "SessionStart": [
                    {
                        "matcher": "*",
                        "hooks": [
                            {"type": "command", "command": "keep-hook"},
                            {"type": "command", "command": module.HOOK},
                        ],
                    }
                ],
            },
        }
        updated = module.claude_hook(original)
        self.assertEqual(module.claude_hook(updated), updated)
        self.assertEqual(
            updated["hooks"]["SessionStart"][0]["hooks"][0]["command"], "keep-hook"
        )
        self.assertEqual(len(original["hooks"]["SessionStart"][0]["hooks"]), 2)

    def test_check_negotiates_then_hides_private_handoff(self):
        with tempfile.TemporaryDirectory() as directory:
            fake = Path(directory) / "server"
            fake.write_text("""#!/usr/bin/env python3
import json, sys
initialized = False
for line in sys.stdin:
 r=json.loads(line)
 if r['method']=='notifications/initialized':
  initialized=True
  continue
 if r['method']=='initialize': result={'protocolVersion':'2025-06-18'}
 elif not initialized: sys.exit(9)
 elif r['method']=='tools/list': result={'tools':[{'name':n} for n in ['sessionmesh_get_current','sessionmesh_get_handoff','sessionmesh_search','sessionmesh_list_sessions','sessionmesh_get_session','sessionmesh_record_decision','sessionmesh_record_task']]}
 else: result={'structuredContent':{'handoff':{'objective':'PRIVATE_SENTINEL'},'delivery':{'origin':'sessionmesh','ingestion_excluded':True}}}
 print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':result}),flush=True)
""")
            fake.chmod(0o700)
            env = {
                **os.environ,
                "SESSIONMESH_MCP_BINARY": str(fake),
                "SESSIONMESH_HOME": directory,
            }
            result = subprocess.run(
                [str(ROOT / "check-sessionmesh-mcp")],
                env=env,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn("PRIVATE_SENTINEL", result.stdout + result.stderr)
            result = subprocess.run(
                [str(ROOT / "sessionmesh-session-start")],
                env=env,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("PRIVATE_SENTINEL", result.stdout)
            self.assertIn("ingestion_excluded", result.stdout)

    def test_bridge_preserves_cwd_and_explicit_permissions(self):
        with tempfile.TemporaryDirectory() as directory:
            fake = Path(directory) / "docker"
            fake.write_text('#!/bin/sh\nprintf "%s\\n" "$@"\n')
            fake.chmod(0o700)
            env = {
                **os.environ,
                "PATH": directory + ":" + os.environ["PATH"],
                "SESSIONMESH_CONTAINER": "custom-container",
                "SESSIONMESH_MCP_ALLOW_WRITES": "false",
            }
            env.pop("SESSIONMESH_MCP_BINARY", None)
            result = subprocess.run(
                [str(ROOT / "sessionmesh-mcp-stdio")],
                env=env,
                cwd=directory,
                text=True,
                capture_output=True,
            )
            self.assertEqual(result.returncode, 0)
            self.assertIn("SESSIONMESH_PROJECT_ROOT=" + directory, result.stdout)
            self.assertIn("SESSIONMESH_MCP_ALLOW_WRITES=false", result.stdout)
            self.assertIn("custom-container", result.stdout)

    def test_hook_rejects_tool_error_instead_of_silent_success(self):
        with tempfile.TemporaryDirectory() as directory:
            fake = Path(directory) / "docker"
            fake.write_text("""#!/usr/bin/env python3
import json, sys
for line in sys.stdin:
 r=json.loads(line)
 if 'id' not in r: continue
 if r['method']=='initialize': result={'protocolVersion':'2025-06-18'}
 elif r['method']=='tools/list': result={'tools':[{'name':n} for n in ['sessionmesh_get_current','sessionmesh_get_handoff','sessionmesh_search','sessionmesh_list_sessions','sessionmesh_get_session','sessionmesh_record_decision','sessionmesh_record_task']]}
 else: result={'isError':True,'content':[{'type':'text','text':'no current global session'}]}
 print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':result}),flush=True)
""")
            fake.chmod(0o700)
            env = {**os.environ, "PATH": directory + ":" + os.environ["PATH"]}
            env.pop("SESSIONMESH_MCP_BINARY", None)
            result = subprocess.run(
                [str(ROOT / "sessionmesh-session-start")],
                env=env,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("no current global session", result.stderr)


if __name__ == "__main__":
    unittest.main()
