"""Reproducible public-target measurement using the unchanged, frozen Rust harness.

Python only controls processes and accounts for their recorded results. It sends no web requests.
"""
import argparse
from collections import Counter
import gzip
import hashlib
import ipaddress
import json
import math
import os
from pathlib import Path
import re
import shutil
import sqlite3
import statistics
import subprocess
import threading
import time
import tomllib

REPO = Path(__file__).resolve().parents[1]
SETS = [('public31', 31), ('hard12', 12), ('vendors8', 8)]


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def load(path):
    return json.loads(Path(path).read_text(encoding='utf-8-sig'))


def save(path, value):
    path = Path(path)
    temporary = path.with_suffix(path.suffix + '.tmp')
    temporary.write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    # Windows readers can briefly deny rename while inspecting progress. Retry only the
    # atomic replacement; the previous complete JSON remains available until it succeeds.
    for attempt in range(10):
        try:
            temporary.replace(path)
            break
        except PermissionError:
            if attempt == 9:
                raise
            time.sleep(min(0.01 * (2 ** attempt), 0.25))


def schedule():
    return [dict(label=f'auto-{name}-{round_number}', set=name, targets=count,
                 round=round_number, seed=20260906 + round_number)
            for round_number in range(1, 4)
            for name, count in SETS[round_number - 1:] + SETS[:round_number - 1]]


def target_sets():
    source = (REPO / 'bench/src/targets.rs').read_text(encoding='utf-8')
    result = {}
    pattern = r't!\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,\s*\[([^\]]*)\]\s*,?\s*\)'
    for name, count in SETS:
        block = source.split(f'pub const {name.upper()}:')[1].split('];', 1)[0]
        rows = [dict(name=n, url=u, wall=w, expect=re.findall(r'"([^"]*)"', e))
                for n, u, w, e in re.findall(pattern, block)]
        if len(rows) != count:
            raise ValueError(f'Target parser found {len(rows)}, expected {count}: {name}')
        result[name] = rows
    return result


def validate_result(data, label, set_name, targets):
    if (data.get('schema') != 1 or data.get('label') != label or
            data.get('set') != set_name or data.get('repeat') != 3 or
            data.get('timeout_ms') != 60000 or not data.get('ended_unix')):
        raise ValueError(f'Incomplete or mismatched measurement: {label}')
    config = tomllib.loads(data.get('config_toml', ''))
    if config.get('browser_identity') != 'auto' or not config.get('auto_native_fallback', True):
        raise ValueError(f'Wrong identity policy: {label}')
    expected = {(target['name'], target['url'], position)
                for target in targets for position in range(1, 4)}
    found = [(c.get('target'), c.get('url'), c.get('position')) for c in data.get('cells', [])]
    if len(found) != len(expected) or set(found) != expected:
        raise ValueError(f'Missing, duplicated or substituted calls: {label}')
    if any(not isinstance(c.get('secs'), (int, float)) or not math.isfinite(c['secs']) or
           c['secs'] < 0 or not isinstance(c.get('delivered'), bool) for c in data['cells']):
        raise ValueError(f'Invalid timing or outcome: {label}')


def summarize_cells(cells):
    delivered = [c for c in cells if c['delivered']]
    times = sorted(c['secs'] for c in cells)
    total = sum(times)
    return dict(
        calls=len(cells), delivered=len(delivered), total_fetch_seconds=total,
        median_call_seconds=statistics.median(times) if times else None,
        p95_call_seconds=times[math.ceil(len(times) * .95) - 1] if times else None,
        delivered_median_seconds=statistics.median(c['secs'] for c in delivered) if delivered else None,
        fetch_seconds_per_delivery=total / len(delivered) if delivered else None,
        refused_without_attempt=sum(not c['delivered'] and c['response'].get('attempts') == [] for c in cells),
        native_attempted=sum(c['response'].get('native_fallback') is True for c in cells),
        native_delivered=sum(c['response'].get('identity_used') == 'native' for c in delivered),
        emulated_delivered=sum(c['response'].get('identity_used') == 'emulated' for c in delivered),
        timeouts=sum('timeout' in str(c['response'].get('stopped_reason', '')).lower() or
                     'timeout' in str(c['response'].get('blocked_reason', '')).lower() for c in cells),
        qualities=dict(Counter(c['response'].get('quality', 'unavailable') for c in cells)),
        outcomes=dict(Counter(c['outcome'] for c in cells)),
    )


def environment(root, browser):
    return dict(os.environ, SVIPALL_HOME=str(root / 'state/home'), SVIPALL_BROWSER=str(browser),
                SVIPALL_HUMAN_ASSIST='0', SVIPALL_HTTP_ENGINE='auto', RUST_LOG='error')


def prepare(args, root):
    if (root / 'manifest.json').exists():
        raise ValueError('Already prepared; use run to resume verified completed batches')
    for directory in ['state/home', 'bin', 'results']:
        (root / directory).mkdir(parents=True, exist_ok=True)
    task_home = root / 'state/home'
    if list(task_home.iterdir()):
        raise ValueError('Preparation refuses to overwrite existing experiment state')
    binary, browser, cli = map(lambda p: Path(p).resolve(), [args.binary, args.browser, args.cli])
    frozen = root / 'bin/auto.exe'
    shutil.copy2(binary, frozen)
    shutil.copy2(root / 'README.md', root / 'protocol.md')
    initial = {}
    source_home = Path(args.seed_home or os.environ.get('SVIPALL_HOME', str(Path.home() / '.svipall')))
    reputation = source_home / 'reputation.json'
    if reputation.exists():
        data = load(reputation)
        if not isinstance(data.get('by_key'), dict):
            raise ValueError('Invalid initial reputation ledger; do not start with a reset budget')
        shutil.copy2(reputation, task_home / 'reputation.json')
        initial['reputation_sha256'] = digest(task_home / 'reputation.json')
    traffic = source_home / 'traffic.sqlite3'
    if traffic.exists():
        with sqlite3.connect(traffic.resolve().as_uri() + '?mode=ro', uri=True) as source:
            with sqlite3.connect(task_home / 'traffic.sqlite3') as target:
                source.backup(target)
        initial['traffic_sha256'] = digest(task_home / 'traffic.sqlite3')
    config = (f'browser_path = {json.dumps(browser.as_posix())}\n'
              'browser_identity = "auto"\nauto_native_fallback = true\nbrowser_auto_install = false\n')
    (task_home / 'config.toml').write_text(config, encoding='utf-8')
    (task_home / 'machine.seed').write_text('75bb21a068def901', encoding='utf-8')
    cfg = json.loads(subprocess.check_output([str(cli), 'config', 'show'],
                                            env=environment(root, browser), encoding='utf-8'))['config']
    if (cfg['auto_max_attempts'], cfg['request_limit'], cfg['request_window_seconds'],
            cfg['request_cooldown_seconds'], cfg['reputation_budget']) != (6, 12, 60, 900, 250):
        raise ValueError('Effective configuration does not retain the default policy limits')
    targets = target_sets()
    save(root / 'targets.json', targets)
    save(root / 'state/paths.json', dict(browser=str(browser), binary=str(frozen)))
    cfg['browser_path'] = '<managed-browser>'
    sources = subprocess.check_output(['git', 'ls-files', '-z', 'Cargo.toml', 'Cargo.lock',
                                      '.cargo', 'crates', 'bench/src'], cwd=REPO).decode().split('\0')
    source_hashes = {p: digest(REPO / p) for p in sources if p and
                     (p.endswith(('.rs', '.toml')) or p == 'Cargo.lock')}
    assets = {p.relative_to(REPO).as_posix(): digest(p)
              for p in (REPO / 'crates/svipall-models/models').iterdir()
              if p.suffix in ['.onnx', '.json']}
    save(root / 'manifest.json', dict(
        schema=1, source_revision=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip(),
        prepared_unix=time.time(), timeout_ms=60000, repeat=3, runs=3, calls=459,
        schedule=schedule(), binary_sha256=digest(frozen), browser_sha256=digest(browser),
        protocol_sha256=digest(root / 'protocol.md'), targets_sha256=digest(root / 'targets.json'),
        effective_config=cfg, initial_state=initial, source_sha256=source_hashes,
        model_asset_sha256=assets, pauses_seconds=120, state='persistent across every batch',
        human_assist=False, rustc=subprocess.check_output(['rustc', '--version'], text=True).strip(),
        build='cargo build --release -p svipall-bench; target-cpu=native; local artifact, not distributable',
    ))
    print('Prepared 459 calls; binary, browser, source, targets and protocol recorded.', flush=True)


def event(root, **data):
    data['at_unix'] = time.time()
    with (root / 'state/events.jsonl').open('a', encoding='utf-8') as stream:
        stream.write(json.dumps(data) + '\n')


def run(root):
    manifest, paths, targets = load(root / 'manifest.json'), load(root / 'state/paths.json'), load(root / 'targets.json')
    binary, browser = Path(paths['binary']), Path(paths['browser'])
    ledger_path = root / 'state/completed.json'
    completed = load(ledger_path) if ledger_path.exists() else []
    finished = {row['label']: row for row in completed}
    event(root, event='controller_started', pid=os.getpid())
    for batch in manifest['schedule']:
        label = batch['label']
        raw_path = root / f'state/{label}.json'
        if label in finished:
            if digest(raw_path) != finished[label]['raw_sha256']:
                raise ValueError(f'Completed raw result changed: {label}')
            validate_result(load(raw_path), label, batch['set'], targets[batch['set']])
            continue
        if raw_path.exists():
            raise ValueError(f'Unrecorded/partial batch exists: {label}; inspect its process before resuming')
        if digest(binary) != manifest['binary_sha256'] or digest(browser) != manifest['browser_sha256']:
            raise ValueError('The frozen executable or browser changed')
        if completed:
            pause_until = completed[-1]['process_ended_unix'] + manifest['pauses_seconds']
            while time.time() < pause_until:
                save(root / 'state/progress.json', dict(state='pause', controller_pid=os.getpid(),
                     completed_calls=sum(r['calls'] for r in completed), until_unix=pause_until, next=label))
                time.sleep(min(15, max(0, pause_until - time.time())))
        command = [str(binary), 'compare', '--set', batch['set'], '--repeat', '3',
                   '--seed', str(batch['seed']), '--label', label, '--timeout', '60000']
        started = time.time()
        latest = {'completed_in_batch': 0, 'last': None}
        with raw_path.open('wb') as stdout:
            process = subprocess.Popen(command, stdout=stdout, stderr=subprocess.PIPE,
                                       env=environment(root, browser), cwd=REPO,
                                       creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
            event(root, event='batch_started', label=label, pid=process.pid,
                  command=['<frozen-binary>', *command[1:]])
            print(f'START {label}; pid={process.pid}', flush=True)

            def collect():
                with (root / f'state/{label}-stderr.txt').open('wb') as log:
                    for line in iter(process.stderr.readline, b''):
                        log.write(line)
                        log.flush()
                        text = line.decode('utf-8', errors='replace').strip()
                        if re.match(r'^\S+ repeat [123]: ', text):
                            latest['completed_in_batch'] += 1
                            latest['last'] = text
                            print(f'{label}: {text}', flush=True)

            reader = threading.Thread(target=collect, daemon=True)
            reader.start()
            deadline = started + batch['targets'] * 3 * 60 + 180
            while process.poll() is None:
                save(root / 'state/progress.json', dict(state='running', controller_pid=os.getpid(),
                     batch=label, child_pid=process.pid, started_unix=started,
                     completed_calls=sum(r['calls'] for r in completed) + latest['completed_in_batch'], **latest))
                if time.time() > deadline:
                    if os.name == 'nt':
                        subprocess.run(['taskkill', '/PID', str(process.pid), '/T', '/F'], check=False,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    else:
                        process.kill()
                    process.wait()
                    raise TimeoutError(f'Batch exceeded all per-call deadlines plus shutdown margin: {label}')
                time.sleep(5)
            reader.join(timeout=30)
        ended = time.time()
        if process.returncode != 0:
            raise RuntimeError(f'{label} exited {process.returncode}; partial output is preserved')
        data = load(raw_path)
        validate_result(data, label, batch['set'], targets[batch['set']])
        row = dict(label=label, calls=len(data['cells']), raw_sha256=digest(raw_path),
                   process_started_unix=started, process_ended_unix=ended,
                   process_seconds=ended - started, exit_code=process.returncode)
        completed.append(row)
        finished[label] = row
        save(ledger_path, completed)
        event(root, event='batch_finished', **row)
        print(f'DONE {label}: {summarize_cells(data["cells"])}', flush=True)
    save(root / 'state/progress.json', dict(state='complete', controller_pid=os.getpid(),
         completed_calls=sum(r['calls'] for r in completed), ended_unix=time.time()))


def discover_ips(records):
    found = set()
    for record in records:
        for cell in record['cells']:
            text = str(cell['response'].get('content', ''))
            for value in re.findall(r'"(?:ip|origin|client_ip|ip_address|ipAddress)"\s*:\s*"([^"]+)"', text):
                for part in value.split(','):
                    for candidate in [part.strip(), part.strip().rsplit(':', 1)[0].strip('[]')]:
                        try:
                            address = ipaddress.ip_address(candidate)
                            if address.is_global:
                                found.add(str(address))
                        except ValueError:
                            pass
    return found


def analyze(root):
    manifest, targets = load(root / 'manifest.json'), load(root / 'targets.json')
    completed = load(root / 'state/completed.json')
    if {r['label'] for r in completed} != {r['label'] for r in manifest['schedule']} or len(completed) != 9:
        raise ValueError('All nine completed batches are required for the final report')
    records = []
    for batch in manifest['schedule']:
        path = root / f'state/{batch["label"]}.json'
        data = load(path)
        validate_result(data, batch['label'], batch['set'], targets[batch['set']])
        if digest(path) != next(row['raw_sha256'] for row in completed if row['label'] == batch['label']):
            raise ValueError('A completed measurement was modified')
        records.append(data)
    ips = discover_ips(records)
    save(root / 'state/ip-redactions.json', sorted(ips))

    def scrub(text):
        for path, replacement in [(str(REPO), '<repo>'), (str(Path.home()), '<home>')]:
            for variant in [json.dumps(path)[1:-1], path, path.replace('\\', '/')]:
                text = text.replace(variant, replacement)
        for address in sorted(ips, key=len, reverse=True):
            text = text.replace(address, '<redacted-exit-ip>')
        return text

    rows = []
    for set_name, target_count in SETS:
        matching = [r for r in records if r['set'] == set_name]
        for position in range(1, 4):
            batches = [[c for c in r['cells'] if c['position'] == position] for r in matching]
            per_round = [summarize_cells(cells) for cells in batches]
            counts = [r['delivered'] for r in per_round]
            rows.append(dict(set=set_name, position=position, targets=target_count, per_round=per_round,
                             median_delivered=statistics.median(counts), min_delivered=min(counts),
                             max_delivered=max(counts), **summarize_cells(sum(batches, []))))
    all_cells = [cell for record in records for cell in record['cells']]
    summaries = dict(rows=rows, overall=summarize_cells(all_cells),
                     calls=len(all_cells), unique_urls=len({c['url'] for c in all_cells}),
                     batch_timings=completed, redacted_exit_addresses=len(ips),
                     controller_wall_seconds=completed[-1]['process_ended_unix'] - completed[0]['process_started_unix'])
    save(root / 'summary.json', summaries)
    hashes = {}
    for record in records:
        path = root / f'results/{record["label"]}.json.gz'
        path.write_bytes(gzip.compress(scrub(json.dumps(record, ensure_ascii=False)).encode('utf-8'), mtime=0))
        hashes[path.name] = digest(path)
        stderr = root / f'state/{record["label"]}-stderr.txt'
        (root / f'results/{record["label"]}-stderr.txt').write_text(scrub(stderr.read_text(encoding='utf-8')), encoding='utf-8')
    save(root / 'verification.json', dict(complete=True, verified_calls=len(all_cells),
         completed_batches=len(completed), raw_hashes_verified=True, published_sha256=hashes,
         protocol_sha256_matches=digest(root / 'protocol.md') == manifest['protocol_sha256'],
         targets_sha256_matches=digest(root / 'targets.json') == manifest['targets_sha256']))
    print(json.dumps(summaries['overall'], indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['prepare', 'run', 'analyze'])
    parser.add_argument('--root', required=True)
    for flag in ['binary', 'browser', 'cli', 'seed-home']:
        parser.add_argument('--' + flag)
    args = parser.parse_args()
    root = Path(args.root).resolve()
    if not root.is_relative_to(REPO / 'bench/experiments'):
        raise ValueError('Experiment root must be inside this repository under bench/experiments')
    if args.action == 'prepare':
        prepare(args, root)
    elif args.action == 'run':
        try:
            run(root)
        except BaseException as error:
            event(root, event='controller_failed', error=str(error))
            save(root / 'state/progress.json', dict(state='failed', controller_pid=os.getpid(), error=str(error)))
            raise
    else:
        analyze(root)


if __name__ == '__main__':
    main()
