#!/usr/bin/env python3
"""Exercise bounded ingestion, HTTP/SSE reads and cleanup using disposable synthetic sessions."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def rss_kib(pid: int) -> int:
    """Read Linux resident memory without inspecting another process's arguments."""
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        if line.startswith("VmRSS:"):
            return int(line.split()[1])
    raise RuntimeError("Daemon exited before RSS could be sampled")


def source_hash(path: Path) -> str:
    """Hash the fixture incrementally, without buffering the source in the harness."""
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def main() -> None:
    """Run only against an isolated temporary home; never inspect real sessions."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--daemon", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--duration", type=int, default=15)
    args = parser.parse_args()
    daemon, cli = args.daemon.resolve(), args.cli.resolve()
    with tempfile.TemporaryDirectory(prefix="sessionmesh-soak-") as directory:
        root = Path(directory)
        state = root / "state"
        codex = root / "codex"
        path = codex / "sessions/2026/07/20/rollout-resource-soak.jsonl"
        path.parent.mkdir(parents=True)
        workspace = root / "workspace"
        workspace.mkdir()
        with socket.socket() as port_socket:
            port_socket.bind(("127.0.0.1", 0))
            port = port_socket.getsockname()[1]
        env = {k: v for k, v in os.environ.items() if not k.startswith("SESSIONMESH_")}
        env.update(
            {
                "CODEX_HOME": str(codex),
                "SESSIONMESH_HOME": str(state),
                "SESSIONMESH_PROFILE_ROOT": str(root / "empty-profiles"),
                "SESSIONMESH_PROFILE_ORIGINAL_ROOT": str(root / "empty-profiles"),
                "SESSIONMESH_BIND_ADDRESS": "127.0.0.1",
                "SESSIONMESH_PORT": str(port),
                "SESSIONMESH_CONVERSATION_ONLY": "true",
                "SESSIONMESH_WATCH_DEBOUNCE_MS": "100",
                "SESSIONMESH_RECONCILE_INTERVAL_MS": "500",
                "SESSIONMESH_MAINTENANCE_INTERVAL_SECS": "60",
                "TOKIO_WORKER_THREADS": "2",
            }
        )

        def record(payload: dict) -> str:
            return (
                json.dumps(
                    {
                        "timestamp": "2026-07-20T14:30:00Z",
                        "type": "response_item",
                        "payload": payload,
                    }
                )
                + "\n"
            )

        with path.open("w") as output:
            output.write(
                json.dumps(
                    {
                        "timestamp": "2026-07-20T14:30:00Z",
                        "type": "session_meta",
                        "payload": {"id": "resource-soak", "cwd": str(workspace)},
                    }
                )
                + "\n"
            )
            for index in range(300):
                for role, content_type in [
                    ("user", "input_text"),
                    ("assistant", "output_text"),
                ]:
                    output.write(
                        record(
                            {
                                "type": "message",
                                "role": role,
                                "content": [
                                    {
                                        "type": content_type,
                                        "text": f"Keep {role} message {index}: Grüße 🌍\n```rust\nfn preserved() {{}}\n```",
                                    }
                                ],
                            }
                        )
                    )
            for index in range(1000):
                output.write(
                    record(
                        {
                            "type": "function_call_output",
                            "call_id": f"tool-{index}",
                            "output": "unnecessary code snapshot\n" * 1024,
                        }
                    )
                )
        with path.open("rb") as source:
            expected_digest = hashlib.file_digest(source, "sha256")
        base = f"http://127.0.0.1:{port}/api/v1"
        samples = []
        with (root / "daemon.log").open("w") as log:
            process = subprocess.Popen([str(daemon)], env=env, stdout=log, stderr=log)
            slow_stream = None
            try:
                deadline = time.monotonic() + 45
                while time.monotonic() < deadline:
                    if process.poll() is not None:
                        raise RuntimeError(
                            "Synthetic daemon failed; "
                            + (root / "daemon.log").read_text()[-1500:]
                        )
                    samples.append(rss_kib(process.pid))
                    try:
                        with urllib.request.urlopen(base + "/health", timeout=1):
                            pass
                        break
                    except urllib.error.URLError:
                        time.sleep(0.1)
                else:
                    raise RuntimeError("Synthetic daemon did not become healthy")
                token = (state / "api-token").read_text()

                def get(endpoint: str):
                    """Make an authenticated fixture request without printing its token or content."""
                    return urllib.request.urlopen(
                        urllib.request.Request(
                            base + endpoint,
                            headers={"Authorization": "Bearer " + token},
                        ),
                        timeout=5,
                    )

                # Wait for all bounded batches, not merely the early conversation records.
                while time.monotonic() < deadline:
                    samples.append(rss_kib(process.pid))
                    with sqlite3.connect(
                        f"file:{state / 'sessionmesh.db'}?mode=ro", uri=True
                    ) as database:
                        cursor = database.execute(
                            "SELECT MAX(byte_offset) FROM ingestion_cursors"
                        ).fetchone()[0]
                    if cursor == path.stat().st_size:
                        break
                    time.sleep(0.1)
                else:
                    raise RuntimeError("Bounded ingestion did not reach the source end")
                baseline = rss_kib(process.pid)
                slow_stream = get("/events/stream")
                slow_stream.readline()
                steady_samples = []
                until = time.monotonic() + max(5, args.duration)
                requests = 0
                added_pairs = 0
                while time.monotonic() < until:
                    for endpoint in ["/native-sessions", "/events?limit=50"]:
                        with get(endpoint) as response:
                            json.load(response)
                        requests += 1
                    if requests % 20 == 0:
                        suffix = "".join(
                            record(
                                {
                                    "type": "message",
                                    "role": role,
                                    "content": [
                                        {
                                            "type": content_type,
                                            "text": f"Keep live {role} output {added_pairs}",
                                        }
                                    ],
                                }
                            )
                            for role, content_type in [
                                ("user", "input_text"),
                                ("assistant", "output_text"),
                            ]
                        )
                        with path.open("a") as output:
                            output.write(suffix)
                        expected_digest.update(suffix.encode("utf-8"))
                        added_pairs += 1
                    steady_samples.append(rss_kib(process.pid))
                    time.sleep(0.1)
                time.sleep(
                    2
                )  # Allow the final source append and handoff refresh to settle.
                subprocess.run(
                    [str(cli), "maintenance", "--apply"],
                    env=env,
                    check=True,
                    capture_output=True,
                    text=True,
                )
                with sqlite3.connect(
                    f"file:{state / 'sessionmesh.db'}?mode=ro", uri=True
                ) as database:
                    counts = dict(
                        database.execute(
                            "SELECT kind,COUNT(*) FROM native_events GROUP BY kind"
                        )
                    )
                    assert (
                        counts.get("UserMessage") == 300 + added_pairs
                        and counts.get("AssistantMessage") == 300 + added_pairs
                    ), counts
                    assert counts.get("ToolResult", 0) == 0, counts
                    assert (
                        database.execute(
                            "SELECT COUNT(*) FROM native_sessions"
                        ).fetchone()[0]
                        == 1
                    ), "Resumed imports split the native session"
                    sample = json.loads(
                        database.execute(
                            "SELECT canonical_json FROM native_events WHERE kind='AssistantMessage' ORDER BY sequence LIMIT 1"
                        ).fetchone()[0]
                    )
                    assert (
                        sample["payload"]["text"]
                        == "Keep assistant message 0: Grüße 🌍\n```rust\nfn preserved() {}\n```"
                    ), "AI output was changed"
                    handoffs = database.execute(
                        "SELECT COUNT(*) FROM handoffs"
                    ).fetchone()[0]
                    assert handoffs <= 1, handoffs
                assert (
                    source_hash(path) == expected_digest.hexdigest()
                ), "Native source was modified"
                state_bytes = sum(
                    item.stat().st_size for item in state.rglob("*") if item.is_file()
                )
                assert (
                    max(steady_samples) - baseline < 64 * 1024
                ), "RSS did not stabilize within the 64 MiB tolerance"
                print(
                    json.dumps(
                        {
                            "source_bytes": path.stat().st_size,
                            "persistent_bytes": state_bytes,
                            "conversation_events": 600 + 2 * added_pairs,
                            "tool_events": 0,
                            "handoffs": handoffs,
                            "requests": requests,
                            "duration_seconds": max(5, args.duration),
                            "warm_rss_kib": baseline,
                            "steady_min_rss_kib": min(steady_samples),
                            "steady_max_rss_kib": max(steady_samples),
                            "sampled_peak_rss_kib": max(samples + steady_samples),
                        },
                        indent=2,
                    )
                )
            finally:
                if slow_stream is not None:
                    slow_stream.close()
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


if __name__ == "__main__":
    main()
