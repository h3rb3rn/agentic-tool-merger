import { execFileSync } from "node:child_process";
import test from "node:test";

test("connector installation and transport regressions", () => {
  execFileSync(
    "python3",
    ["-m", "unittest", "discover", "-s", "scripts", "-p", "test_connectors.py"],
    { stdio: "pipe", env: { ...process.env, PYTHONDONTWRITEBYTECODE: "1" } },
  );
});
