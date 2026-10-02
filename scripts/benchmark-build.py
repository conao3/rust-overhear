import argparse
import json
import pathlib
import shutil
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser(description="Nix のキャッシュ境界と変更後のビルド時間を検証する")
parser.add_argument("--build", action="store_true", help="Rust / UI の変更版もビルドして時間を測る")
args = parser.parse_args()
repo = pathlib.Path(__file__).resolve().parents[1]
common = subprocess.check_output(
    ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"], cwd=repo, text=True
).strip()
scratch = pathlib.Path(common).parent / ".claude-dev" / "scratch" / "build-cache"
scratch.mkdir(parents=True, exist_ok=True)
run = pathlib.Path(tempfile.mkdtemp(prefix="benchmark-", dir=scratch))
snapshot = run / "source"
snapshot.mkdir()
for name in subprocess.check_output(["git", "ls-files", "-z"], cwd=repo).decode().split("\0"):
    if not name or not (repo / name).is_file():
        continue
    destination = snapshot / name
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(repo / name, destination)


def paths(ref):
    result = subprocess.check_output(
        ["nix", "eval", "--json", ref + "#packages.x86_64-linux", "--apply",
         "p: { app = p.default.drvPath; deps = p.rust-deps.drvPath; frontend = p.frontend.drvPath; }"],
        cwd=repo, text=True,
    )
    return json.loads(result)


ref = "path:" + str(snapshot)
baseline = paths(ref)
assert baseline == paths("."), "検証用コピーと作業ツリーのビルドが一致しない"
results = {"baseline": baseline}
print(f"検証ログ: {run}", flush=True)


def build(name):
    log = run / (name + ".log")
    start = time.monotonic()
    with log.open("w") as output:
        subprocess.run(
            ["nix", "build", "--option", "cores", "4", "--max-jobs", "1", "-L", ref,
             "--out-link", str(run / (name + "-result"))],
            cwd=repo, stdout=output, stderr=subprocess.STDOUT, check=True,
        )
    duration = round(time.monotonic() - start, 2)
    print(f"{name}: {duration}s ({log})", flush=True)
    return duration


if args.build:
    results["baseline_seconds"] = build("baseline")

cases = [
    ("docs", "README.md", "\nNix build cache benchmark.\n"),
    ("rust", "crates/overhear-core/src/lib.rs", f'\nconst _: &str = "{run.name}";\n'),
    ("ui", "src/index.css", f'\n:root {{ --build-cache-benchmark: "{run.name}"; }}\n'),
    ("manifest", "Cargo.toml", '\n[profile.release]\noverflow-checks = true\n'),
]
for name, filename, addition in cases:
    path = snapshot / filename
    original = path.read_text()
    try:
        path.write_text(original + addition)
        current = paths(ref)
        if name == "docs":
            assert current == baseline, "ドキュメント変更で再ビルドされる"
        elif name == "manifest":
            assert current["deps"] != baseline["deps"], "Cargo 設定変更が依存キャッシュへ反映されない"
        else:
            assert current["deps"] == baseline["deps"], f"{name}: 依存キャッシュが無効化される"
            assert current["app"] != baseline["app"], f"{name}: アプリ変更が反映されない"
            assert (current["frontend"] == baseline["frontend"]) == (name == "rust")
        results[name] = {"paths": current}
        print(f"{name}: キャッシュ境界 OK", flush=True)
        if args.build and name in ("rust", "ui"):
            results[name]["seconds"] = build(name)
    finally:
        path.write_text(original)
    (run / "results.json").write_text(json.dumps(results, ensure_ascii=False, indent=2) + "\n")
print(f"結果: {run / 'results.json'}", flush=True)
