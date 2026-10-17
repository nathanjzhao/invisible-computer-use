#!/usr/bin/env python3
"""Read-only export of the exact observer integration inputs from taiku Git objects."""
import argparse
import hashlib
import io
import json
import subprocess
import tarfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
RELEASE = 'bb87e4301e'
PROJECTS = ['taiku-observer', 'taiku-observer-schema', 'taiku-observer-audio-helper']


def export(repository, out):
    if out.exists():
        raise SystemExit('use a fresh export directory')
    def git(*args):
        return subprocess.check_output(['git', '-C', str(repository), *args])
    archive = git('archive', RELEASE, *('deploy/' + p for p in PROJECTS))
    out.mkdir(parents=True)
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        tar.extractall(out, filter='data')
    source = out / 'deploy'
    overrides = json.loads((HERE / 'pinned-source-overrides.json').read_text())
    for item in overrides:
        data = git('show', item['revision'] + ':deploy/' + item['path'])
        assert hashlib.sha256(data).hexdigest() == item['sha256'], item['path']
        (source / item['path']).write_bytes(data)
    clicks = json.loads((HERE / 'manifest.json').read_text())['files']
    original = {item['path']: item for item in clicks}
    checks = []
    for manifest in ['manifest.json', 'audio-manifest.json']:
        for item in json.loads((HERE / manifest).read_text())['files']:
            expected = item['base_sha256']
            if manifest == 'audio-manifest.json' and item['path'] in original:
                assert expected == original[item['path']]['patched_sha256']
                expected = original[item['path']]['base_sha256']
            path = source / item['path']
            actual = hashlib.sha256(path.read_bytes()).hexdigest() if path.exists() else None
            assert actual == expected, f'{manifest}: {item["path"]}'
            checks.append({'manifest': manifest, 'path': item['path'], 'sha256': actual})
    for name, expected in json.loads((HERE / 'audio-helper-manifest.json').read_text())['files'].items():
        assert hashlib.sha256((source / 'taiku-observer-audio-helper' / name).read_bytes()).hexdigest() == expected, name
    receipt = {'release': git('rev-parse', RELEASE).decode().strip(), 'archive_sha256': hashlib.sha256(archive).hexdigest(),
               'overrides': overrides, 'base_checks': checks,
               'files': {str(p.relative_to(source)): hashlib.sha256(p.read_bytes()).hexdigest()
                         for p in sorted(source.rglob('*')) if p.is_file()}}
    (out / 'export-manifest.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(source)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repository', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    export(args.repository, args.out)
