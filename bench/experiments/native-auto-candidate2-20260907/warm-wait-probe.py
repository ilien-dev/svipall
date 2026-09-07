"""Local-only evidence for the proposed short-page warm-wait optimization."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = Path(__file__).resolve().parent
EXTRACT = ROOT.parent / 'native-auto-extraction-20260907'
BINARY = EXTRACT / 'bin/before.exe'
assert hashlib.sha256(BINARY.read_bytes()).hexdigest() == json.loads(
    (EXTRACT / 'local-before.json').read_text(encoding='utf-8'))['binary_sha256']
ARTICLE = '<main><h1>Rendered report</h1><p>' + (
    'The completed report contains the requested observations and their context. '
    'Researchers measured independent samples and recorded the experimental method. '
    'These results are now available for inspection alongside the measurements. '
) + '</p></main>'
PADDING = '<style>/*' + 'x' * 7000 + '*/</style>'
PAGES = {
    '/static': '<html><head>' + PADDING + '</head><body><main>Short useful answer.</main></body></html>',
    '/delayed': '<html><head>' + PADDING + '</head><body><div id="root">Loading...</div>'
        '<script>setTimeout(() => { document.body.innerHTML=' + json.dumps(ARTICLE)
        + '; }, 3500);</script></body></html>',
}


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        payload = PAGES.get(self.path, '<html><body>Local fixture</body></html>').encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *args):
        pass


server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
rows = []
try:
    for shape, mode in [('static', 'warm'), ('delayed', 'real'), ('delayed', 'warm')]:
        home = ROOT / 'state' / ('warm-probe-' + shape + '-' + mode)
        home.mkdir(parents=True, exist_ok=False)
        (home / 'config.toml').write_text(
            'browser_identity = "native"\nmax_tier = "warm"\nblock_ads = false\n'
            'warm_wait_ms = 6000\nwarm_max_wait_ms = 6000\nwarm_adaptive = false\n',
            encoding='utf-8')
        env = {k: v for k, v in os.environ.items() if not k.startswith('SVIPALL_')}
        env.update(SVIPALL_HOME=str(home), SVIPALL_HUMAN_ASSIST='0',
            SVIPALL_BROWSER='C:/Users/jesus/.svipall/browser/cft/152.0.7977.75/chrome-win64/chrome.exe')
        start = time.time()
        result = subprocess.run([str(BINARY), 'fetch',
            f'http://127.0.0.1:{server.server_port}/{shape}', '--mode', mode,
            '--cache', 'bypass', '--timeout', '30000'], env=env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60,
            creationflags=subprocess.CREATE_NO_WINDOW)
        (home / 'stdout.json').write_bytes(result.stdout)
        (home / 'stderr.txt').write_bytes(result.stderr)
        value = json.loads(result.stdout)
        content = value.get('content', '')
        row = dict(shape=shape, mode=mode, exit_code=result.returncode,
            seconds=time.time()-start, status=value.get('status'),
            tier_used=value.get('tier_used'), identity_used=value.get('identity_used'),
            warm=value.get('warm'), content=content,
            completed_report='completed report contains' in content)
        rows.append(row)
        print(json.dumps(row), flush=True)
finally:
    server.shutdown()
(ROOT / 'warm-wait-probe.json').write_text(json.dumps(dict(
    binary_sha256=hashlib.sha256(BINARY.read_bytes()).hexdigest(), rows=rows,
    scope='Local fixtures only; shortened wait for diagnosis, not the public protocol.'
), indent=2) + '\n', encoding='utf-8')
