#!/usr/bin/env python3
"""Record exact workspace state + binary bytes used by a verification run."""
import hashlib, json, subprocess, sys
from pathlib import Path
from datetime import datetime, timezone
ROOT = Path(__file__).resolve().parents[1]
def git(*args): return subprocess.check_output(['git', *args], cwd=ROOT, text=True)
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
paths = [Path(p).resolve() for p in sys.argv[1:]]
for p in paths:
    if not p.is_file(): raise SystemExit(f'required binary missing: {p}')
source_files = sorted(set(git('ls-files', '--cached', '--others', '--exclude-standard', '--', 'impl', 'conformance').splitlines()))
source_files = [p for p in source_files if Path(ROOT/p).is_file() and not '/target/' in p and not '__pycache__' in p]
vendor_root = ROOT/'vendor-cua'
vendor_files = sorted(set(git('-C', 'vendor-cua', 'ls-files', '--cached', '--others', '--exclude-standard', '--', 'libs/cua-driver/rust').splitlines()))
vendor_files = [p for p in vendor_files if (vendor_root/p).is_file() and '/target/' not in p]
print(json.dumps({'time_utc': datetime.now(timezone.utc).isoformat(), 'revision': git('rev-parse', 'HEAD').strip(),
    'state': git('status', '--short'), 'vendor_revision': git('-C', 'vendor-cua', 'rev-parse', 'HEAD').strip(),
    'vendor_diff_sha256': hashlib.sha256(git('-C', 'vendor-cua', 'diff', '--', 'libs/cua-driver/rust/crates').encode()).hexdigest(), 'diff_sha256': hashlib.sha256(git('diff', 'HEAD', '--', 'impl', 'conformance').encode()).hexdigest(),
    'source_sha256': {p: sha(ROOT/p) for p in source_files},
    'vendor_source_sha256': {p: sha(vendor_root/p) for p in vendor_files},
    'binaries': {str(p): sha(p) for p in paths}}, indent=2))
