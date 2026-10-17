"""Verify staging from a clean directory with a fake build, never a desktop driver."""
import os
from pathlib import Path
import shutil
import subprocess


def test_stages_built_driver_when_binaries_directory_does_not_exist(tmp_path):
    app = tmp_path / "app"
    (app / "scripts").mkdir(parents=True)
    source = Path(__file__).resolve().parents[1] / "scripts/stage-driver.sh"
    script = app / "scripts/stage-driver.sh"
    shutil.copyfile(source, script)
    native = tmp_path / "impl/native"
    native.mkdir(parents=True)
    (native / "apply-and-build.sh").write_text("#!/bin/sh\nexit 0\n")
    (native / "macos-sign.sh").write_text("icu_sign_macos_binary() { return 0; }\n")
    built = tmp_path / "vendor-cua/libs/cua-driver/rust/target/release/cua-driver"
    built.parent.mkdir(parents=True)
    built.write_bytes(b"fixture binary bytes")
    built.chmod(0o755)
    commands = tmp_path / "commands"
    commands.mkdir()
    for name, body in {
        "rustc": "printf 'host: x86_64-unknown-linux-gnu\\n'",
        "uname": "printf 'Linux\\n'",
    }.items():
        command = commands / name
        command.write_text(f"#!/bin/sh\n{body}\n")
        command.chmod(0o755)
    env = os.environ.copy()
    env["PATH"] = str(commands) + os.pathsep + env["PATH"]
    subprocess.run(["bash", str(script)], env=env, check=True, capture_output=True)
    staged = app / "src-tauri/binaries/cua-driver-x86_64-unknown-linux-gnu"
    assert staged.read_bytes() == built.read_bytes()
    assert os.access(staged, os.X_OK)


def _windows_stage(tmp_path, with_worker):
    app = tmp_path / "app"
    (app / "scripts").mkdir(parents=True)
    source = Path(__file__).resolve().parents[1] / "scripts/stage-driver.sh"
    script = app / "scripts/stage-driver.sh"
    shutil.copyfile(source, script)
    native = tmp_path / "impl/native"
    native.mkdir(parents=True)
    (native / "apply-and-build.sh").write_text("#!/bin/sh\nexit 0\n")
    (native / "macos-sign.sh").write_text("icu_sign_macos_binary() { return 0; }\n")
    release = tmp_path / "vendor-cua/libs/cua-driver/rust/target/release"
    release.mkdir(parents=True)
    (release / "cua-driver.exe").write_bytes(b"driver bytes")
    if with_worker:
        (release / "cua-driver-uia.exe").write_bytes(b"uiaccess worker bytes")
    commands = tmp_path / "commands"
    commands.mkdir()
    for name, body in {
        "rustc": "printf 'host: x86_64-pc-windows-msvc\\n'",
        "uname": "printf 'MINGW64_NT-10.0-20348\\n'",
    }.items():
        command = commands / name
        command.write_text(f"#!/bin/sh\n{body}\n")
        command.chmod(0o755)
    env = os.environ.copy()
    env["PATH"] = str(commands) + os.pathsep + env["PATH"]
    result = subprocess.run(["bash", str(script)], env=env, capture_output=True, text=True)
    return app / "src-tauri/binaries", result


def test_windows_stages_the_uiaccess_worker_beside_the_driver(tmp_path):
    # tauri.windows.conf.json lists both sidecars; the bundle (and so the signed
    # installer) only carries the worker if staging put it there.
    binaries, result = _windows_stage(tmp_path, with_worker=True)
    assert result.returncode == 0, result.stderr
    assert (binaries / "cua-driver-x86_64-pc-windows-msvc.exe").read_bytes() == b"driver bytes"
    assert (binaries / "cua-driver-uia-x86_64-pc-windows-msvc.exe").read_bytes() == b"uiaccess worker bytes"


def test_windows_staging_fails_without_the_uiaccess_worker(tmp_path):
    binaries, result = _windows_stage(tmp_path, with_worker=False)
    assert result.returncode != 0
    assert "UIAccess worker not built" in result.stderr
    assert not (binaries / "cua-driver-uia-x86_64-pc-windows-msvc.exe").exists()


def test_windows_config_bundles_both_sidecars():
    import json
    config = json.loads((Path(__file__).resolve().parents[1] / "src-tauri/tauri.windows.conf.json").read_text())
    assert config["bundle"]["externalBin"] == ["binaries/cua-driver", "binaries/cua-driver-uia"]
    # UIAccess needs a secure location (Program Files): the installer must offer a per-machine install.
    assert config["bundle"]["windows"]["nsis"]["installMode"] in ("both", "perMachine")
