#!/usr/bin/env python3
"""Single local/CI parity entrypoint. Every mandatory producer fails closed."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import subprocess as sp
import sys
import time
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", help="New run directory; never reuses a previous pass")
    parser.add_argument("--require-clean", action="store_true", help="Require a committed, clean source candidate (CI/release)")
    args = parser.parse_args()
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + secrets.token_hex(3)
    output = Path(args.output).resolve() if args.output else ROOT / "target/parity/runs" / run_id
    output.mkdir(parents=True, exist_ok=False)
    logs = output / "logs"
    logs.mkdir()
    env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT / "target"), PYTHONDONTWRITEBYTECODE="1",
               RUSTUP_TOOLCHAIN=tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"])
    env.pop("NODE_TEST_CONTEXT", None)
    env.pop("VELOCIMD_UI_PROBE_PATH", None)
    env["RUSTFLAGS"] = ""
    env["CARGO_ENCODED_RUSTFLAGS"] = ""
    if not env.get("TMPDIR"):
        env["TMPDIR"] = str(output / "tmp")
        Path(env["TMPDIR"]).mkdir()
    bindgen = env.get("WASM_BINDGEN") or shutil.which("wasm-bindgen") or str(ROOT / "target/tools/wasm-bindgen-cli/bin/wasm-bindgen")
    env["WASM_BINDGEN"] = bindgen
    for tool in ("cargo", "rustc", "node", "pnpm", "Xvfb", "xvfb-run", "xdotool", "import", "python3"):
        if shutil.which(tool) is None:
            raise RuntimeError("Missing prerequisite: " + tool + ". See docs/browser-parity.md")
    assert Path(bindgen).is_file(), "Install the Cargo.lock-matched wasm-bindgen CLI into target/tools/wasm-bindgen-cli, or set WASM_BINDGEN"
    expected = next(p["version"] for p in tomllib.loads((ROOT / "Cargo.lock").read_text())["package"] if p["name"] == "wasm-bindgen")
    actual = sp.check_output([bindgen, "--version"], env=env, text=True).strip()
    assert actual == "wasm-bindgen " + expected, "wasm-bindgen CLI differs from Cargo.lock"
    package = json.loads((ROOT / "package.json").read_text())
    assert sp.check_output(["node", "--version"], env=env, text=True).strip() == "v" + package["engines"]["node"], "Use the pinned Node version"
    assert sp.check_output(["pnpm", "--version"], env=env, text=True).strip() == package["packageManager"].split("@")[-1], "Use the pinned pnpm version"
    versions = {name: sp.check_output(command, env=env, cwd=ROOT, text=True).strip() for name, command in {
        "rustc": ["rustc", "--version"], "cargo": ["cargo", "--version"], "node": ["node", "--version"],
        "pnpm": ["pnpm", "--version"], "wasmBindgen": [bindgen, "--version"],
    }.items()}
    versions["wasmBindgenSha256"] = digest(bindgen)
    report = {"schema": 1, "status": "running", "runId": run_id, "gates": [], "tools": versions,
              "consoleErrors": [], "matrix": {}, "assets": {}}
    report_path = output / "verification.json"

    def save():
        report_path.write_text(json.dumps(report, indent=2) + "\n")

    def run(gate_id, command, timeout=900, extra_env=None):
        out, err = logs / (gate_id + ".stdout"), logs / (gate_id + ".stderr")
        print("RUN", gate_id, flush=True)
        started = time.monotonic()
        result = {"id": gate_id, "status": "running", "argv": command, "exitCode": None}
        report["gates"].append(result)
        save()
        try:
            with out.open("w") as stdout, err.open("w") as stderr:
                completed = sp.run(command, cwd=ROOT, env=dict(env, **(extra_env or {})),
                                   stdout=stdout, stderr=stderr, timeout=timeout, text=True)
            result["exitCode"] = completed.returncode
            assert completed.returncode == 0, f"{gate_id} failed (exit {completed.returncode}); see {err} and {out}"
            result["status"] = "passed"
        except BaseException as error:
            result["status"] = "failed"
            result["error"] = str(error)
            raise
        finally:
            result["durationMs"] = round((time.monotonic() - started) * 1000)
            result["stdoutSha256"] = digest(out)
            result["stderrSha256"] = digest(err)
            save()
        print("PASS", gate_id, flush=True)
        return result

    try:
        sp.run(["node", "scripts/parity/source-record.mjs", str(output / "source.json")], cwd=ROOT, env=env, check=True)
        source = json.loads((output / "source.json").read_text())
        report["sourceSha256"] = source["sha256"]
        if args.require_clean or env.get("GITHUB_ACTIONS") == "true":
            assert not source["modifiedBrowserBuild"], "CI/release refuses a dirty source candidate"
        run("architecture", ["cargo", "test", "--locked", "--test", "ui_parity_contract"])
        run("rust-fmt", ["cargo", "fmt", "--all", "--check"])
        run("rust-test", ["cargo", "test", "--locked"])
        run("rust-clippy", ["cargo", "clippy", "--locked", "--all-targets", "--all-features", "--", "-D", "warnings"])
        run("native-build", ["cargo", "build", "--locked", "--release", "--bin", "velocimd"])
        run("wasm-build", ["bash", "scripts/build-web.sh"])
        tests = sorted(str(p.relative_to(ROOT)) for p in (ROOT / "scripts/parity").glob("*.test.mjs"))
        assert len(tests) >= 3, "Missing verifier unit-test module"
        gate = run("tooling-tests", ["node", "--test", "--test-reporter=tap", *tests])
        tap = (logs / "tooling-tests.stdout").read_text()
        totals = {key: int(value) for key, value in re.findall(r"^# (tests|pass|fail|cancelled|skipped|todo) (\d+)$", tap, re.M)}
        assert set(totals) == {"tests", "pass", "fail", "cancelled", "skipped", "todo"}, "Incomplete Node test summary"
        assert totals["tests"] > 0 and totals["pass"] == totals["tests"] and all(totals[key] == 0 for key in ("fail", "cancelled", "skipped", "todo")), "Nonpassing/empty verifier suite"
        gate["totals"] = totals
        gate["modules"] = tests
        native = run("native-ui", [sys.executable, "scripts/parity/native.py", "--binary", str(ROOT / "target/release/velocimd"), "--output", str(output / "native")], timeout=240)
        native_report = json.loads((output / "native-report.json").read_text())
        native["cases"] = [{"id": item["id"], "status": "passed"} for item in native_report["cases"] + native_report["checks"]]
        native["caseCount"] = len(native["cases"])
        # Both browser engines need a real software-GL display. Never the user desktop.
        browser_env = {"VELOCIMD_PARITY_OWNED_DISPLAY": "1", "LIBGL_ALWAYS_SOFTWARE": "1", "GALLIUM_DRIVER": "llvmpipe"}
        run("browser-matrix-producer", ["xvfb-run", "-a", "-s", "-screen 0 1500x1000x24", "node", "scripts/parity/browser.mjs", "--web", str(ROOT / "target/web"), "--source", str(output / "source.json"), "--output", str(output / "browsers")], extra_env=browser_env, timeout=600)
        matrix = json.loads((output / "browsers/matrix.json").read_text())
        assert matrix["status"] == "passed" and len(matrix["reports"]) == 2
        for browser in matrix["reports"]:
            cases = [{"id": case["id"], "status": "passed"} for case in browser["cases"] + browser["checks"]]
            report["gates"].append({"id": "browser-" + browser["engine"], "status": browser["status"], "exitCode": 0, "cases": cases, "caseCount": len(cases), "version": browser["version"]})
        report["assets"] = matrix["reports"][0]["assets"]
        assert report["assets"] == matrix["reports"][1]["assets"], "Browsers tested different bytes"
        for browser_name in ("chromium", "firefox"):
            regression = run("browser-regressions-" + browser_name, ["xvfb-run", "-a", "-s", "-screen 0 1500x1000x24", "node", "scripts/test-web.mjs"], timeout=480, extra_env=dict(browser_env, BROWSER=browser_name, HEADED="1", EVIDENCE_DIR=str(output / ("regressions-" + browser_name)), **({"CHROME_DEVEL_SANDBOX": "/opt/google/chrome/chrome-sandbox"} if Path("/opt/google/chrome/chrome-sandbox").is_file() else {})))
            receipt = json.loads((output / ("regressions-" + browser_name) / "results.json").read_text())
            assert receipt["browserName"] == browser_name and receipt["consoleErrors"] == [], "Invalid browser safety receipt"
            regression["cases"] = [{"id": row["name"], "status": row["status"]} for row in receipt["results"]]
            regression["caseCount"] = len(regression["cases"])
            regression["version"] = receipt["browser"]
        visual = run("visual-parity", ["node", "scripts/parity/compare.mjs", "--native", str(output / "native-report.json"), "--matrix", str(output / "browsers/matrix.json"), "--output", str(output / "visual")])
        visual_report = json.loads((output / "visual/report.json").read_text())
        visual["cases"] = [{"id": case["id"], "status": case["status"]} for case in visual_report["cases"]]
        visual["caseCount"] = len(visual["cases"])
        report["gates"].append({"id": "negative-controls", "status": "passed", "exitCode": 0, "cases": visual_report["negativeControls"], "caseCount": len(visual_report["negativeControls"])})
        report["matrix"] = {"browsers": [browser["engine"] for browser in matrix["reports"]], "caseIds": [case["id"] for case in native_report["cases"]]}
        report["status"] = "passed"
        save()
        # This command rehashes current inputs before packaging and independently validates the distribution.
        sp.run(["node", "scripts/parity/bundle.mjs", "--source", str(output / "source.json"), "--report", str(report_path), "--output", str(output / "distribution")], cwd=ROOT, env=env, check=True)
        run("distribution-round-trip", ["node", "scripts/parity/test-distribution.mjs", str(output / "distribution/artifact.json")], timeout=180)
        report["distributionChecks"] = json.loads((output / "distribution/post-package-checks.json").read_text())
        save()
        print(json.dumps({"status": "passed", "sourceSha256": source["sha256"], "report": str(report_path), "distribution": str(output / "distribution")}), flush=True)
    except BaseException as error:
        report["status"] = "failed"
        report["error"] = str(error)
        save()
        raise


if __name__ == "__main__":
    main()
