#!/usr/bin/python3
"""Record attempts to invoke an unbound Tk fallback in the owned scope probe."""
import json
import os
from pathlib import Path
import sys

path = Path(os.environ['SCOPE_TRAP_OUTPUT'])
path.write_text(json.dumps({'argv':sys.argv, 'script':sys.stdin.read()}, indent=2) + '\n')
raise SystemExit(1)
