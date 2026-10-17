#!/usr/bin/env python3
"""Build an ad-hoc-signed acceptance bundle without installing or launching it.

Uses verified sidecars and a vendor tree with the tracked patch. Build sources are
copied into an immutable candidate snapshot; no signing credentials are used.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
EXCLUDED = {"target", "node_modules", "dist", "binaries", ".git", "__pycache__", ".pytest_cache", ".DS_Store", ".venv", "gen"}
PRIVATE_NAMES = {".adminpw", "auth.json", "credentials.json", "id_rsa", "id_ed25519"}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def ignore(directory, names):
    return [name for name in names if name in EXCLUDED or name in PRIVATE_NAMES or name.startswith(".env")]


def git_ignored(source):
    """Honor repository exclusions while keeping new, nonignored source files."""
    result = subprocess.run(["git", "-C", str(source), "rev-parse", "--show-toplevel"],
                            capture_output=True, text=True)
    if result.returncode: return set()
    paths = []
    file_paths = []
    for directory, dirs, files in os.walk(source, followlinks=False):
        skipped = set(ignore(directory, dirs + files))
        dirs[:] = [name for name in dirs if name not in skipped]
        paths.extend(str(Path(directory) / name) for name in dirs + files if name not in skipped)
        file_paths.extend(Path(directory)/name for name in files if name not in skipped)
    if not paths: return set()
    checked = subprocess.run(["git", "-C", str(source), "check-ignore", "-z", "--stdin"],
                             input="\0".join(paths).encode()+b"\0", capture_output=True)
    if checked.returncode not in (0, 1): raise RuntimeError("cannot establish source export exclusions")
    ignored = {Path(os.fsdecode(path)) for path in checked.stdout.split(b"\0") if path}
    retained = [path for path in file_paths if path not in ignored]
    # An ignored directory can still contain explicitly tracked source files.
    return {path for path in ignored if path.is_symlink() or not path.is_dir()
            or not any(file.is_relative_to(path) for file in retained)}


def copy_source(source, destination):
    """Refuse links rather than accidentally following a mutable external checkout."""
    if source.is_symlink(): raise RuntimeError(f"source snapshot refuses symlink: {source}")
    source = source.resolve()
    ignored = git_ignored(source)
    def selected_ignore(directory, names):
        return list(set(ignore(directory, names)) | {name for name in names if Path(directory)/name in ignored})
    for directory, dirs, files in os.walk(source, followlinks=False):
        skipped = set(selected_ignore(directory, dirs + files))
        dirs[:] = [name for name in dirs if name not in skipped]
        for name in dirs + [name for name in files if name not in skipped]:
            if (Path(directory) / name).is_symlink():
                raise RuntimeError(f"source snapshot refuses symlink: {Path(directory) / name}")
    shutil.copytree(source, destination, ignore=selected_ignore)


def source_hashes(source):
    result = {}
    for directory, dirs, files in os.walk(source, followlinks=False):
        skipped = set(ignore(directory, dirs + files))
        dirs[:] = [name for name in dirs if name not in skipped]
        for name in files:
            if name not in skipped:
                path = Path(directory) / name
                if path.is_symlink(): raise RuntimeError("mutable link appeared in candidate sources")
                result[str(path.relative_to(source))] = digest(path)
    return dict(sorted(result.items()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", required=True, type=Path)
    parser.add_argument("--vendor", required=True, type=Path)
    parser.add_argument("--driver", required=True, type=Path)
    parser.add_argument("--driver-sha256", required=True)
    parser.add_argument("--observer", type=Path, help="Optional source-scoped audio recorder to bundle")
    parser.add_argument("--observer-sha256", help="Required pinned hash when --observer is supplied")
    parser.add_argument("--audio-helper", type=Path, help="Required system audio helper when --observer is supplied")
    parser.add_argument("--audio-helper-sha256")
    parser.add_argument("--fixture", type=Path, help="Optional already-compiled owned fixture to pin in the manifest")
    parser.add_argument("--ui", type=Path, help="Optional already-compiled macOS UI probe to pin in the manifest")
    parser.add_argument("--target-dir", type=Path, default=ROOT / "app/src-tauri/target")
    parser.add_argument("--prepare-only", action="store_true", help="Freeze sources and verify sidecars without compiling")
    args = parser.parse_args()
    if sys.platform != "darwin": raise SystemExit("macOS bundle preparation requires macOS")
    if digest(args.driver) != args.driver_sha256: raise SystemExit("driver differs from the verified input hash")
    if args.observer and (not args.observer_sha256 or digest(args.observer)!=args.observer_sha256): raise SystemExit("observer differs from its required input hash")
    if args.observer and (not args.audio_helper or not args.audio_helper_sha256 or digest(args.audio_helper)!=args.audio_helper_sha256): raise SystemExit("the macOS observer requires its pinned system audio helper")
    subprocess.run(["git", "-C", str(args.vendor), "-c", "core.autocrlf=false", "apply", "--check", "--reverse",
                    str(ROOT / "impl/native/cua-driver.patch")], check=True)
    args.work.mkdir(parents=True, exist_ok=True)
    source = args.work / "source"
    if source.exists(): raise SystemExit("use a fresh work directory; never blend candidate sources")
    source.mkdir()
    app = source / "app"
    copy_source(ROOT / "app", app)
    copy_source(ROOT / "impl", source / "impl")
    vendor_rust = Path("vendor-cua/libs/cua-driver/rust")
    (source / vendor_rust).parent.mkdir(parents=True)
    copy_source(args.vendor / "libs/cua-driver/rust", source / vendor_rust)
    pinned_sources = source_hashes(source)
    vendor_head = subprocess.check_output(["git", "-C", str(args.vendor), "rev-parse", "HEAD"], text=True).strip()
    (args.work / "source-manifest.json").write_text(json.dumps({"vendor_head":vendor_head,"source_sha256":pinned_sources}, indent=2)+"\n")
    binaries = app / "src-tauri/binaries"; binaries.mkdir()
    staged = binaries / "cua-driver-aarch64-apple-darwin"
    shutil.copy2(args.driver, staged)
    if digest(staged) != args.driver_sha256: raise SystemExit("driver changed during staging")
    subprocess.run(["codesign", "--force", "--sign", "-", "--timestamp=none", str(staged)], check=True)
    if args.observer:
        observer=binaries / "taiku-observer-aarch64-apple-darwin"
        shutil.copy2(args.observer,observer)
        if digest(observer) != args.observer_sha256: raise SystemExit("observer changed during staging")
        subprocess.run(["codesign", "--force", "--sign", "-", "--timestamp=none", str(observer)], check=True)
        helper=binaries / "taiku-observer-audio-helper-aarch64-apple-darwin"
        shutil.copy2(args.audio_helper,helper)
        if digest(helper) != args.audio_helper_sha256: raise SystemExit("audio helper changed during staging")
        subprocess.run(["codesign", "--force", "--sign", "-", "--timestamp=none", str(helper)], check=True)
    config = {"build": {"beforeBuildCommand": "bash scripts/check-icons.sh && npm run build"},
              "bundle": {"targets": ["app"], "createUpdaterArtifacts": False, "macOS": {"signingIdentity": "-"}},
              "plugins": {"updater": {"endpoints": ["https://127.0.0.1:18765/latest.json"]}}}
    if args.observer: config["bundle"]["externalBin"]=["binaries/cua-driver","binaries/taiku-observer","binaries/taiku-observer-audio-helper"]
    overlay = args.work / "acceptance-config.json"; overlay.write_text(json.dumps(config, indent=2) + "\n")
    if args.prepare_only:
        print(json.dumps({"status":"sources_frozen","source_files":len(pinned_sources),"source":str(source)}))
        return
    with (args.work / "npm-ci.log").open("w") as log:
        subprocess.run(["npm", "ci", "--ignore-scripts", "--offline"], cwd=app, stdout=log, stderr=subprocess.STDOUT, check=True)
    env = {key: value for key, value in os.environ.items() if not key.startswith("APPLE_")}
    rustc = subprocess.check_output(["rustup", "which", "--toolchain", "1.97.1", "rustc"], text=True).strip()
    env.update(PATH=str(Path(rustc).parent) + os.pathsep + env["PATH"], RUSTC=rustc, RUSTC_WRAPPER="",
               CARGO_TARGET_DIR=str(args.target_dir.resolve()), CARGO_NET_OFFLINE="true", APPLE_SIGNING_IDENTITY="-")
    command = [str(app / "node_modules/.bin/tauri"), "build", "--bundles", "app", "--config", str(overlay.resolve()), "--", "--offline", "--locked"]
    with (args.work / "build.log").open("w") as log:
        result = subprocess.run(command, cwd=app, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=900)
    if result.returncode: raise SystemExit(f"bundle build failed; inspect {args.work / 'build.log'}")
    if source_hashes(source) != pinned_sources:
        raise SystemExit("candidate sources changed during the build; discard this build")
    bundle = args.work / "Copilot.app"
    shutil.copytree(args.target_dir / "release/bundle/macos/Copilot.app", bundle)
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(bundle)], check=True)
    native = bundle / "Contents/MacOS"
    manifest = {"schema": 2, "status": "built_not_live_tested", "signing": "ad-hoc; not notarized",
                "app_sha256": digest(native / "app"), "driver_sha256": digest(native / "cua-driver"),
                "driver_input_sha256": args.driver_sha256,
                "probe_sha256": digest(ROOT / "app/tests/probe_macos_real_model.py"),
                "macos_probe_sha256": digest(ROOT / "app/tests/macos_probe.py"),
                "verdict_sha256": digest(ROOT / "app/tests/real_model_verdict.py"),
                "task_quality_sha256": digest(ROOT / "app/tests/task_quality.py"),
                "suite_sha256": digest(ROOT / "app/tests/run_macos_real_model_suite.py"),
                "source_sha256": pinned_sources, "vendor_head":vendor_head,"acceptance_config": config,
                "frozen_local_sources":True,"npm_install":"npm ci --ignore-scripts --offline"}
    if args.fixture is not None: manifest["fixture_sha256"] = digest(args.fixture)
    if args.observer is not None:
        manifest["observer_input_sha256"]=args.observer_sha256
        manifest["observer_sha256"]=digest(native / "taiku-observer")
        manifest["audio_helper_input_sha256"]=args.audio_helper_sha256
        manifest["audio_helper_sha256"]=digest(native / "taiku-observer-audio-helper")
    if args.ui is not None: manifest["ui_sha256"] = digest(args.ui)
    (args.work / "build-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({key: value for key, value in manifest.items() if key != "source_sha256"}, indent=2))


if __name__ == "__main__": main()
