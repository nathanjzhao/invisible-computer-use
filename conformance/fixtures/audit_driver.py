#!/usr/bin/env python3
"""Isolated transport fixture. Never connects to a real driver or desktop."""
import json, os, sys, time
from pathlib import Path

mode = os.environ.get('AUDIT_DRIVER_MODE', 'healthy')
Path(os.environ['AUDIT_DRIVER_PID']).write_text(str(os.getpid()))
# A slow start, as a cold Python under load gives (nested_runtime.rs).
time.sleep(int(os.environ.get('AUDIT_DRIVER_START_DELAY_MS', '0')) / 1000)
tools = ['list_windows', 'list_apps', 'get_window_state', 'click', 'type_text', 'health_report']
if mode == 'missing_click': tools.remove('click')
health = {'overall': 'ok', 'platform': 'darwin', 'driver_version': '0.24.0', 'checks': [
    {'name': 'tcc_accessibility', 'status': 'pass'},
    {'name': 'tcc_screen_recording', 'status': 'pass'},
    {'name': 'host_interactive', 'status': 'pass', 'data': {'host': {
        'screen_locked': False, 'on_console': True, 'display_asleep': False}}}]}
if mode == 'old_version': health['driver_version'] = '0.1.0'
if mode == 'unknown_version': health.pop('driver_version')
if mode == 'malformed_version': health['driver_version'] = 'nightly'
if mode == 'prerelease_version': health['driver_version'] = '0.24.0-rc.1'
for line in sys.stdin:
    req = json.loads(line)
    if 'id' not in req: continue
    method = req['method']
    if mode == 'eof': break
    if mode == 'silent_init' or mode == 'silent_health' and method == 'tools/call':
        time.sleep(60)
    if mode in ('garbage', 'notifications', 'wrong_id', 'partial') and method == 'initialize':
        while True:
            msg = {'jsonrpc': '2.0', 'method': 'notifications/progress'} if mode == 'notifications' else {'id': 999, 'result': {}}
            sys.stdout.write('x' * 4096 if mode == 'partial' else ('garbage' if mode == 'garbage' else json.dumps(msg)) + '\n')
            sys.stdout.flush()
    if method == 'initialize':
        result = {'protocolVersion': '2024-11-05', 'capabilities': {}, 'serverInfo': {'name': 'fixture', 'version': '1'}}
    elif method == 'tools/list':
        if mode == 'unknown_tools':
            print(json.dumps({'jsonrpc': '2.0', 'id': req['id'], 'error': {'code': -32601, 'message': 'discovery unavailable'}}), flush=True)
            continue
        result = {} if mode == 'malformed_tools' else {'tools': [{'name': n} for n in tools]}
    else:
        name = req['params']['name']
        payload = health if name == 'health_report' else {'apps': [], 'windows': []}
        if mode == 'blocked_write' and name == 'health_report': payload['padding'] = 'x' * 500000
        result = {'structuredContent': payload}
    print(json.dumps({'jsonrpc': '2.0', 'id': req['id'], 'result': result}), flush=True)
    if mode == 'blocked_write' and method == 'initialize':
        # Handshake succeeds, then the reader stops; the next large request fills the pipe.
        time.sleep(60)
