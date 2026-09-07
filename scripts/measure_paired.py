"""Serial matched-arm controller. Network work belongs to the frozen Rust product harness."""
import argparse
import json
import math
import os
from pathlib import Path
import random
import shutil
import sqlite3
import statistics
import subprocess
import time
import zipfile

from measure_automatic import REPO, digest, load, save, target_sets

LIMIT_FILES = ('reputation.json', 'cooldowns.json', 'traffic.sqlite3')


def schedule():
    slots = [dict(set=s, target=t['name'], url=t['url'])
             for s, targets in target_sets().items() for t in targets]
    leading = {(s['set'], s['target']): i % 2 for i, s in enumerate(slots)}
    rows = []
    for round_number in range(1, 4):
        order = slots.copy()
        random.Random(2026090600 + round_number).shuffle(order)
        for slot in order:
            first = (leading[(slot['set'], slot['target'])] + round_number - 1) % 2
            arms = ['auto', 'native'] if first == 0 else ['native', 'auto']
            for position, arm in enumerate(arms):
                rows.append(dict(**slot, arm=arm, round=round_number, order=position, repeat=3,
                    label=f'r{round_number}-{slot["set"]}-{slot["target"]}-{arm}'))
    return rows


def summary(cells):
    passes = [c for c in cells if c['delivered']]
    seconds = sum(c['secs'] for c in cells)
    return dict(calls=len(cells), delivered=len(passes), seconds=seconds,
                median_seconds=statistics.median(c['secs'] for c in cells) if cells else None,
                successful_median_seconds=statistics.median(c['secs'] for c in passes) if passes else None,
                seconds_per_delivery=seconds / len(passes) if passes else None)


def validate_result(data, batch):
    if any(data.get(k) != v for k, v in dict(schema=2, label=batch['label'], arm=batch['arm'],
            set=batch['set'], target=batch['target'], repeat=batch['repeat'], timeout_ms=60000).items()):
        raise ValueError('Batch metadata mismatch')
    if not data.get('ended_unix') or data.get('effective_config', {}).get('browser_identity') != batch['arm']:
        raise ValueError('Incomplete batch or wrong identity')
    config = data['effective_config']
    limits = dict(auto_native_fallback=True, auto_max_attempts=6, request_limit=12,
                  request_window_seconds=60, request_cooldown_seconds=900, reputation_budget=250)
    if any(config.get(k) != v for k, v in limits.items()):
        raise ValueError('Policy limits changed')
    cells = data.get('cells', [])
    if sorted(c.get('position', 0) for c in cells) != list(range(1, batch['repeat'] + 1)):
        raise ValueError('Missing or duplicated calls')
    target = next(t for t in target_sets()[batch['set']] if t['name'] == batch['target'])
    for c in cells:
        if c.get('url') != target['url'] or c.get('target') != target['name']:
            raise ValueError('Target substitution')
        if not isinstance(c.get('secs'), (float, int)) or not math.isfinite(c['secs']) or c['secs'] < 0:
            raise ValueError('Invalid timing')
        response = c['response']
        content = str(response.get('content') or '')
        expected = not target['expect'] or any(e.lower() in content.lower() for e in target['expect'])
        status = response.get('status') or 0
        delivered = 200 <= status < 400 and response.get('blocked_reason') is None and bool(content.strip()) and expected
        if c.get('delivered') != delivered or c.get('expected') != expected:
            raise ValueError('Delivery check does not match returned content')
        if batch['arm'] == 'native':
            if response.get('tier_used') == 'http' or any(str(a).startswith('http:') for a in response.get('attempts', [])):
                raise ValueError('Standalone native arm used HTTP')
            if delivered and response.get('identity_used') != 'native':
                raise ValueError('Standalone native delivery has the wrong identity')
        if response.get('native_fallback') and not response.get('privacy_notice'):
            raise ValueError('Missing native exposure notice')
        chunks = c.get('chunks', [])
        if not chunks or any(x.get('from_cache') is not True or x.get('stale_cursor') is True for x in chunks[1:]):
            raise ValueError('Continuation fetched again or changed the document')


def copy_limits(source, destination):
    """Move the current shared accounting forward; never restore an earlier experiment snapshot."""
    destination.mkdir(parents=True, exist_ok=True)
    for name in LIMIT_FILES:
        path = source / name
        if not path.exists():
            if (destination / name).exists():
                raise ValueError(f'Refusing to lose an existing limit file: {name}')
            continue
        if name.endswith('.sqlite3'):
            with sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True) as src:
                with sqlite3.connect(destination / name) as dst:
                    src.backup(dst)
        else:
            shutil.copy2(path, destination / name)


def source_hashes():
    # Include unstaged tests, fixtures, compiled assets and QC documentation as well as code.
    # Respect ignore rules and exclude prior experiment artifacts from the build-input snapshot.
    names = subprocess.check_output(
        ['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'],
        cwd=REPO).decode().split('\0')
    return {n: digest(REPO / n) for n in sorted(set(names))
            if n and not n.startswith('bench/experiments/')}


def environment(home, browser):
    env = {k: v for k, v in os.environ.items() if not k.startswith('SVIPALL_')}
    return dict(env, SVIPALL_HOME=str(home), SVIPALL_BROWSER=str(browser),
                SVIPALL_HUMAN_ASSIST='0', SVIPALL_HTTP_ENGINE='auto', RUST_LOG='error')


def prepare(root, binary, browser, seed_home):
    if (root / 'manifest.json').exists():
        raise ValueError('Already prepared')
    for directory in ['state/limits', 'state/home-auto', 'state/home-native', 'bin', 'results']:
        (root / directory).mkdir(parents=True, exist_ok=True)
    if list((root / 'state/limits').iterdir()):
        raise ValueError('Existing accounting must not be overwritten')
    copy_limits(seed_home, root / 'state/limits')
    # Route evidence, cookies and browser profiles remain arm-local. The shared accounting is
    # copied only after a child has shut down, so JSON replacement and SQLite WALs cannot race.
    config = 'browser_auto_install = false\nbrowser_path = ' + json.dumps(browser.as_posix()) + '\n'
    for arm in ['auto', 'native']:
        home = root / f'state/home-{arm}'
        (home / 'config.toml').write_text(config, encoding='utf-8')
        (home / 'machine.seed').write_text('75bb21a068def901', encoding='utf-8')
    frozen = root / 'bin/baseline.exe'
    shutil.copy2(binary, frozen)
    runtime_source = REPO / 'bench/experiments/automatic-public-20260906/bin'
    for dll in runtime_source.glob('*.dll'):
        shutil.copy2(dll, root / 'bin' / dll.name)
    hashes = source_hashes()
    with zipfile.ZipFile(root / 'source-before.zip', 'w', zipfile.ZIP_DEFLATED) as archive:
        for name in hashes:
            archive.write(REPO / name, name)
    save(root / 'targets.json', target_sets())
    save(root / 'state/paths.json', dict(browser=str(browser), binary=str(frozen), seed_home=str(seed_home)))
    save(root / 'manifest.json', dict(schema=1, variant='baseline', calls=918, repeat=3, rounds=3,
        revision=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip(),
        source_sha256=hashes, schedule=schedule(), prepared_unix=time.time(),
        binary_sha256=digest(frozen), browser_sha256=digest(browser),
        runtime_sha256={p.name: digest(p) for p in (root / 'bin').glob('*.dll')},
        protocol_sha256=digest(root / 'protocol.md'), targets_sha256=digest(root / 'targets.json'),
        initial_limits_sha256={p.name: digest(p) for p in (root / 'state/limits').iterdir() if p.is_file()},
        seed_state='Final accounting from the preceding public experiment; no budget or cooldown reset.',
        pauses_between_rounds_seconds=120, timeout_ms=60000,
        native='browser_identity=native, mode=warm', auto='browser_identity=auto, mode=auto',
        cache='write on each visit, read only for fresh cached markdown continuations',
        state='Persistent separate arm homes; shared traffic, cooldown and reputation accounting.',
        build='cargo build --release -p svipall-bench; target-cpu=native; local use only'))


def run(root):
    manifest, paths = load(root / 'manifest.json'), load(root / 'state/paths.json')
    binary, browser = Path(paths['binary']), Path(paths['browser'])
    completed_path = root / 'state/completed.json'
    completed = load(completed_path) if completed_path.exists() else []
    finished = {r['label']: r for r in completed}
    for batch in manifest['schedule']:
        label = batch['label']
        raw = root / f'state/{label}.json'
        if label in finished:
            if digest(raw) != finished[label]['raw_sha256']:
                raise ValueError('Recorded raw result changed')
            validate_result(load(raw), batch)
            continue
        if raw.exists():
            raise ValueError(f'Partial or unrecorded batch exists: {label}. Inspect its process before recovery.')
        if digest(binary) != manifest['binary_sha256'] or digest(browser) != manifest['browser_sha256']:
            raise ValueError('Frozen executable changed')
        if completed and batch['round'] != completed[-1]['round']:
            until = completed[-1]['ended_unix'] + manifest['pauses_between_rounds_seconds']
            while time.time() < until:
                save(root / 'state/progress.json', dict(state='pause', controller_pid=os.getpid(), until_unix=until,
                    completed_calls=sum(r['calls'] for r in completed), next=label))
                time.sleep(min(5, max(0, until - time.time())))
        home = root / f'state/home-{batch["arm"]}'
        copy_limits(root / 'state/limits', home)
        command = [str(binary), 'paired', '--set', batch['set'], '--target', batch['target'],
            '--arm', batch['arm'], '--repeat', str(batch['repeat']), '--timeout', '60000', '--label', label]
        started = time.time()
        with raw.open('wb') as out, (root / f'state/{label}-stderr.txt').open('wb') as err:
            child = subprocess.Popen(command, cwd=REPO, env=environment(home, browser), stdout=out, stderr=err,
                creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
            print(f'START {label} pid={child.pid}', flush=True)
            while child.poll() is None:
                save(root / 'state/progress.json', dict(state='running', controller_pid=os.getpid(), child_pid=child.pid,
                    batch=label, started_unix=started, completed_calls=sum(r['calls'] for r in completed)))
                # Includes 15 seconds for local continuations per call and shutdown margin.
                if time.time() > started + batch['repeat'] * 75 + 120:
                    subprocess.run(['taskkill', '/PID', str(child.pid), '/T', '/F'], check=False,
                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    child.wait()
                    copy_limits(home, root / 'state/limits')
                    raise TimeoutError(f'Batch deadline exceeded: {label}; partial result retained')
                time.sleep(1)
        copy_limits(home, root / 'state/limits')
        if child.returncode:
            raise RuntimeError(f'{label} exited {child.returncode}; retained')
        data = load(raw)
        validate_result(data, batch)
        row = dict(label=label, round=batch['round'], calls=len(data['cells']), raw_sha256=digest(raw),
                   started_unix=started, ended_unix=time.time(), arm=batch['arm'])
        completed.append(row)
        save(completed_path, completed)
        print(f'DONE {label}: {json.dumps(summary(data["cells"]))}', flush=True)
    save(root / 'state/progress.json', dict(state='complete', controller_pid=os.getpid(),
        completed_calls=sum(r['calls'] for r in completed), ended_unix=time.time()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['prepare', 'run'])
    parser.add_argument('--root', required=True, type=Path)
    for name in ['binary', 'browser', 'seed-home']:
        parser.add_argument('--' + name, type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    if not root.is_relative_to(REPO / 'bench/experiments'):
        raise ValueError('Experiment must live under bench/experiments')
    if args.action == 'prepare':
        prepare(root, args.binary.resolve(), args.browser.resolve(), args.seed_home.resolve())
    else:
        try:
            run(root)
        except BaseException as error:
            save(root / 'state/progress.json', dict(state='failed', controller_pid=os.getpid(),
                 ended_unix=time.time(), error=str(error)))
            raise


if __name__ == '__main__':
    main()
