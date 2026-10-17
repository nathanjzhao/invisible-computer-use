#!/usr/bin/env python3
"""Diagnostic ICU_CUA_BIN: record every request the app sends its owned driver.

Started by the app as `<this> serve [--no-overlay] --socket PATH`. It runs the
real driver (ICU_TAP_REAL_DRIVER) on PATH.real, listens on PATH itself and
relays bytes both ways, appending each app->driver line to PATH.requests.jsonl
with a timestamp. With ICU_TAP_RESPONSES=1 each driver->app line goes to
PATH.responses.jsonl too, image data dropped. Diagnostics only: the owned-driver identity checks do not
hold while it is interposed.
"""
import json, os, socket, subprocess, sys, threading, time

args = sys.argv[1:]
path = args[args.index('--socket') + 1]
real = path + '.real'
log = open(path + '.requests.jsonl', 'a', buffering=1)
responses = open(path + '.responses.jsonl', 'a', buffering=1) if os.environ.get('ICU_TAP_RESPONSES') == '1' else None
driver = subprocess.Popen([os.environ.get('ICU_TAP_REAL_DRIVER', sys.argv[0].replace('driver_socket_tap.py', 'cua-driver')),
                           *[real if a == path else a for a in args]])
for _ in range(200):
    if os.path.exists(real): break
    time.sleep(.05)
if os.path.exists(path): os.unlink(path)
server = socket.socket(socket.AF_UNIX); server.bind(path); server.listen(16)


def without_images(value):
    if isinstance(value, dict):
        return {k: ('<dropped>' if k in ('data', 'png_b64') and isinstance(v, str) and len(v) > 256 else without_images(v)) for k, v in value.items()}
    if isinstance(value, list): return [without_images(v) for v in value]
    return value


def pump(src, dst, record):
    buf = b''
    try:
        while True:
            chunk = src.recv(1 << 20)
            if not chunk: break
            if record is None and responses:
                buf += chunk
                while b'\n' in buf:
                    line, buf = buf.split(b'\n', 1)
                    try: responses.write(json.dumps({'time': time.time(), 'response': without_images(json.loads(line))}) + '\n')
                    except ValueError: pass
            elif record:
                buf += chunk
                while b'\n' in buf:
                    line, buf = buf.split(b'\n', 1)
                    try: log.write(json.dumps({'time': time.time(), 'request': json.loads(line)}) + '\n')
                    except ValueError: log.write(json.dumps({'time': time.time(), 'raw': line[:200].decode(errors='replace')}) + '\n')
            dst.sendall(chunk)
    except OSError: pass
    finally:
        for s in (src, dst):
            try: s.shutdown(socket.SHUT_RDWR)
            except OSError: pass


def serve():
    while True:
        client, _ = server.accept()
        upstream = socket.socket(socket.AF_UNIX); upstream.connect(real)
        threading.Thread(target=pump, args=(client, upstream, True), daemon=True).start()
        threading.Thread(target=pump, args=(upstream, client, None), daemon=True).start()


threading.Thread(target=serve, daemon=True).start()
sys.exit(driver.wait())
