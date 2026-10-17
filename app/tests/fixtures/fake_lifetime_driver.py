#!/usr/bin/python3
"""Socket-only owned child. No desktop input or IP networking."""
import os
import sys
import time

def startup_trace(stage):
    if os.environ.get('ICU_FIXTURE_TRACE'):
        print(f'fixture {os.getpid()} {stage} {time.time_ns()}', file=sys.stderr, flush=True)

startup_trace('entered')
import base64
import json
import pathlib
import socket
import threading

startup_trace('imported')

directory = pathlib.Path(__file__).parent

PNG = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=')
if (directory/'image.png').exists():
    PNG = (directory/'image.png').read_bytes()

def observation(args=None):
    if args and args.get('screenshot_out_file'):
        output_path = pathlib.Path(args['screenshot_out_file'])
        existed_before = output_path.exists()
        output_path.write_bytes(PNG)
        with (directory/'capture-outputs.jsonl').open('a') as output:
            output.write(json.dumps({'path':str(output_path), 'parent_mode':output_path.parent.stat().st_mode & 0o777,
                'existed_before':existed_before, 'bytes':output_path.stat().st_size})+'\n')
    configured = directory/'observation.json'
    if configured.exists():
        result = json.loads(configured.read_text())
        count_file = directory/'capture-count'
        count = int(count_file.read_text()) + 1 if count_file.exists() else 1
        count_file.write_text(str(count))
        result['screenshot_id'] = f'fixture-capture-{count}'
        if (directory/'strict-observation-bindings').exists():
            # Stress capture/snapshot churn between an agent read and its action.
            # This fixture deliberately advances both generations on preview too.
            result['snapshot_id'] = f'fixture-snapshot-{count}'
            for element in result.get('elements', []):
                element['element_token'] = f'fixture-token-{count}'
        return result
    return {'window_bounds':{'width':1280,'height':960},
        'screenshot_width':640,'screenshot_height':480,
        'elements':[{'element_index':0,'role':'text_area','label':'Editor','element_token':'fixture-token'}]}

def observation_reply(args):
    fields = observation(args)
    content = []
    if args.get('screenshot_out_file'):
        fields['screenshot_file_path'] = args['screenshot_out_file']
    elif args.get('include_screenshot') is not False:
        content.append({'type':'image','mimeType':'image/png','data':base64.b64encode(PNG).decode()})
        if (directory/'auxiliary-surface').exists():
            # Mirror native inline capture: an overlay adds extra capture work
            # and pixels; file-output deliberately preserves primary-only scope.
            with (directory/'auxiliary-captures').open('a') as output:
                output.write('captured auxiliary surface\n')
            content.append({'type':'image','mimeType':'image/png','data':base64.b64encode(PNG).decode()})
            fields['screenshots'] = [{'id':'target'},{'id':'unrelated-overlay'}]
    return {'structuredContent':fields,'content':content}

def hold(gate):
    # Lifecycle tests keep a received request in flight until the gate is removed.
    while (directory/gate).exists():
        time.sleep(.02)

def permissions():
    configured = directory/'permissions.json'
    return json.loads(configured.read_text()) if configured.exists() else {'accessibility':True,'screen_recording':True}

def health():
    configured = directory/'health.json'
    return json.loads(configured.read_text()) if configured.exists() else {'platform':sys.platform}

if len(sys.argv) >= 6 and sys.argv[1] == '--socket' and sys.argv[3] == 'call':
    with (directory/'requests.jsonl').open('a') as output:
        output.write(json.dumps({'name':sys.argv[4],'args':json.loads(sys.argv[5]),'transport':'cli'})+'\n')
    if sys.argv[4] == 'click':
        # The app reaches this only after a pre-dispatch socket failure.
        # Preserve the native result envelope rather than legacy flattened text.
        assert os.environ.get('CUA_CLI_RESULT_ENVELOPE') == '1', 'action fallback omitted the result envelope'
        result = json.loads((directory/'click-error.json').read_text())
        print(json.dumps({'cua_cli_result_envelope':1,'result':result}))
        sys.exit(0)
    if sys.argv[4] == 'check_permissions':
        print(json.dumps(permissions()))
        sys.exit(0)
    if sys.argv[4] == 'health_report':
        result = health()
        print(json.dumps(result))
        sys.exit(1 if result.get('isError') else 0)
    assert sys.argv[4] == 'get_window_state', 'unexpected fixture CLI tool'
    (directory/'cli-observe.pid').write_text(str(os.getpid()))
    hold('observe-gate')
    print(json.dumps(observation(json.loads(sys.argv[5]))))
    sys.exit(0)

# Stand in for a slow start (cold imports on a loaded host) when a test run asks for one.
time.sleep(float(os.environ.get('ICU_FIXTURE_START_DELAY', '0')))
path = pathlib.Path(sys.argv[-1])
(directory/'startup.json').write_text(json.dumps({'argv':sys.argv[1:]}))
sessions = {}
# Match the embedded daemon: its owner must remove a proved stale endpoint.
server = socket.socket(socket.AF_UNIX)
server.bind(str(path))
startup_trace('bound')
bound = path.stat()
server.listen()

def accept():
    while True:
        connection, _ = server.accept()
        with connection:
            for line in connection.makefile('rb'):
                request = json.loads(line)
                if request['method'] == 'metadata':
                    metadata = json.loads((directory/'metadata.json').read_text())
                    response = {'ok':True,'result':dict(metadata,pid=os.getpid(),embedded=True)}
                else:
                    name = request.get('name'); session = request.get('session_id')
                    with (directory/'requests.jsonl').open('a') as output:
                        output.write(json.dumps({'pid':os.getpid(),'name':name,'session':session,'args':request.get('args')})+'\n')
                    response = {'ok':True,'result':{}}
                    if name == 'check_permissions': response = {'ok':True,'result':permissions()}
                    elif name == 'health_report': response = {'ok':True,'result':health()}
                    elif name == 'list_windows': sessions.setdefault(session,False)
                    elif session not in sessions: response = {'ok':False,'error':'session not initialized'}
                    elif name == 'set_agent_cursor_enabled': sessions[session] = request['args'].get('enabled') is False
                    elif not sessions[session]: response = {'ok':False,'error':'cursor was not hidden before input'}
                    elif name == 'get_window_state' and (request['args'].get('include_screenshot') is not False or request['args'].get('screenshot_out_file')) and (directory/'capture-refusal.json').exists():
                        # A typed per-window capture refusal inside a successful envelope.
                        response = {'ok':True,'result':json.loads((directory/'capture-refusal.json').read_text())}
                    elif name == 'get_window_state' and hold('observe-gate') is None: response = {'ok':True,'result':observation_reply(request['args'])}
                    elif name == 'click' and (directory/'strict-observation-bindings').exists():
                        current = (directory/'capture-count').read_text()
                        args = request['args']
                        if args.get('screenshot_id') is not None:
                            stale = args['screenshot_id'] != f'fixture-capture-{current}'
                            code = 'stale_capture'
                        else:
                            stale = args.get('element_token') != f'fixture-token-{current}'
                            code = 'stale_element_token'
                        if stale:
                            response = {'ok':True,'result':{'isError':True,
                                'content':[{'type':'text','text':f'refused ({code}): fixture changed; observe again'}],
                                'structuredContent':{'code':code,'effect':'none'}}}
                        else:
                            with (directory/'delivered-clicks.jsonl').open('a') as output:
                                output.write(json.dumps(args)+'\n')
                    elif name == 'click' and (directory/'click-error.json').exists():
                        # Native tool failure is inside a successful daemon envelope.
                        response = {'ok':True,'result':json.loads((directory/'click-error.json').read_text())}
                    elif name == 'type_text' and (directory/'type-refusal.json').exists():
                        # A typed driver refusal, answered once; the next attempt is delivered.
                        response = {'ok':True,'result':json.loads((directory/'type-refusal.json').read_text())}
                        (directory/'type-refusal.json').unlink()
                    elif name == 'type_text':
                        hold('input-gate')
                        with (directory/'received-text.txt').open('a') as output:
                            output.write(request['args']['text'])
                    elif name == 'scroll':
                        # All native scroll implementations require direction.
                        if request['args'].get('direction') not in ('up','down','left','right'):
                            response = {'ok':False,'error':'Missing required string field direction.'}
                connection.sendall((json.dumps(response)+'\n').encode())

threading.Thread(target=accept, daemon=True).start()
if os.environ.get('CUA_DRIVER_PARENT_LIVENESS_STDIN') == '1':
    sys.stdin.buffer.read()
    time.sleep(.1)
    path.with_suffix('.cleaned').write_text('cleanup completed')
    if path.exists():
        current = path.stat()
        if (current.st_dev, current.st_ino) == (bound.st_dev, bound.st_ino):
            path.unlink()
else:
    while True:
        time.sleep(1)
