"""Execute the bounded same-document check in public-html-protocol.md, once."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[2]
CANDIDATE = ROOT.parent / 'native-auto-candidate2-20260907'
PREVIOUS = ROOT.parent / 'native-auto-candidate1-20260907'
sys.path.insert(0, str(REPO / 'scripts'))
from measure_automatic import digest, load, save, target_sets
from measure_paired import copy_limits, environment


def execute(command, env, stem, stdin=None):
    started = time.time()
    with stem.with_suffix('.json').open('wb') as out, stem.with_suffix('.stderr.txt').open('wb') as err:
        child = subprocess.Popen(command, cwd=REPO, env=env, stdout=out, stderr=err,
            stdin=subprocess.PIPE if stdin is not None else subprocess.DEVNULL,
            creationflags=subprocess.CREATE_NO_WINDOW)
        timed_out = False
        try:
            child.communicate(input=stdin, timeout=90)
        except subprocess.TimeoutExpired:
            timed_out = True
            subprocess.run(['taskkill', '/PID', str(child.pid), '/T', '/F'], check=True,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            child.wait()
    return dict(exit_code=child.returncode, timed_out=timed_out, seconds=time.time()-started,
        metadata_sha256=digest(stem.with_suffix('.json')),
        stderr_sha256=digest(stem.with_suffix('.stderr.txt')))


def main():
    reviewed = load(ROOT / 'capture-readiness.json')
    assert reviewed['allow_same_document_capture'] is True
    assert reviewed['corpus_before_manifest_sha256'] == digest(ROOT / 'corpus-before-manifest.json')
    assert load(ROOT / 'corpus-before-manifest.json')['exit_code'] == 0
    assert reviewed['local_regressions_reviewed'] is True
    assert '=== extraction floors ===' in (CANDIDATE / 'state/qc-raw.txt').read_text(
        encoding='utf-8', errors='replace'), 'Wait until the default timed CPU gate has finished'
    assert not (CANDIDATE / 'manifest.json').exists(), 'Capture must precede the next public freeze'
    assert load(PREVIOUS / 'state/progress.json')['state'] == 'complete'
    binaries = {arm: ROOT / f'bin/{arm}.exe' for arm in ['before', 'after']}
    for arm, binary in binaries.items():
        assert digest(binary) == load(ROOT / f'local-{arm}.json')['binary_sha256']
    browser = Path(load(PREVIOUS / 'state/paths.json')['browser'])
    assert digest(browser) == load(PREVIOUS / 'manifest.json')['browser_sha256']
    state = ROOT / 'state/public-html'
    state.mkdir(exist_ok=False)
    home = state / 'home'
    copy_limits(PREVIOUS / 'state/limits', home)
    (home / 'config.toml').write_text(
        'browser_identity = "native"\nbrowser_auto_install = false\n'
        'browser_path = ' + json.dumps(browser.as_posix()) + '\n'
        'request_limit = 12\nrequest_window_seconds = 60\nrequest_cooldown_seconds = 900\n'
        'request_min_interval_ms = 1000\nreputation_budget = 250\n', encoding='utf-8')
    (home / 'machine.seed').write_text('75bb21a068def901', encoding='utf-8')
    env = environment(home, browser)
    targets = {t['name']: t for group in target_sets().values() for t in group}
    record = dict(started_unix=time.time(), protocol_sha256=digest(ROOT / 'public-html-protocol.md'),
        readiness_sha256=digest(ROOT / 'capture-readiness.json'),
        binaries={arm: digest(binary) for arm, binary in binaries.items()},
        initial_limits={p.name: digest(p) for p in home.iterdir()
            if p.name in ['reputation.json', 'cooldowns.json', 'traffic.sqlite3']}, calls=[])
    for name in ['github-explore', 'indeed-jobs', 'google-search']:
        save(state / 'progress.json', dict(state='capturing', target=name, completed=len(record['calls'])))
        html = state / (name + '.html')
        call = dict(target=name, capture=execute([str(binaries['before']), 'fetch', targets[name]['url'],
            '--mode', 'warm', '--extraction', 'html', '--cache', 'bypass', '--timeout', '60000',
            '--out', str(html)], env, state / (name + '-capture')))
        # Preserve accounting even on a failed capture; the next comparison must seed from here.
        copy_limits(home, state / 'limits')
        value = {}
        try:
            value = load(state / (name + '-capture.json'))
        except (ValueError, OSError):
            pass
        call['capture'].update(status=value.get('status'), identity_used=value.get('identity_used'),
            tier_used=value.get('tier_used'), blocked=bool(value.get('blocked_reason')),
            html_bytes=html.stat().st_size if html.exists() else 0,
            html_sha256=digest(html) if html.exists() else None)
        call['replays'] = []
        if html.exists() and html.stat().st_size:
            for arm, binary in binaries.items():
                replay_home = state / ('replay-home-' + arm)
                replay_home.mkdir(exist_ok=True)
                (replay_home / 'config.toml').write_text('browser_auto_install = false\n', encoding='utf-8')
                for full in [False, True]:
                    label = f'{name}-{arm}-' + ('full' if full else 'main')
                    output = state / (label + '.md')
                    command = [str(binary), 'fetch', 'raw:', '--stdin', '--out', str(output)]
                    if full:
                        command.append('--full')
                    result = execute(command, environment(replay_home, browser), state / label,
                        stdin=html.read_bytes())
                    content = output.read_text(encoding='utf-8') if output.exists() else ''
                    call['replays'].append(dict(arm=arm, full=full, **result,
                        markdown_sha256=digest(output) if output.exists() else None,
                        characters=len(content), headings=len(re.findall(r'^#{1,6} ', content, re.M)),
                        links=len(re.findall(r'\]\(', content))))
            full_rows = [r for r in call['replays'] if r['full']]
            call['full_outputs_equal'] = (len(full_rows) == 2 and
                full_rows[0]['markdown_sha256'] is not None and
                full_rows[0]['markdown_sha256'] == full_rows[1]['markdown_sha256'])
        record['calls'].append(call)
        save(ROOT / 'public-html-check.json', record)
        print(json.dumps(dict(target=name, capture=call['capture'],
            replays=len(call['replays']), full_outputs_equal=call.get('full_outputs_equal'))), flush=True)
    record['ended_unix'] = time.time()
    record['final_limits'] = {p.name: digest(p) for p in (state / 'limits').iterdir() if p.is_file()}
    save(ROOT / 'public-html-check.json', record)
    save(state / 'progress.json', dict(state='complete', completed=len(record['calls'])))


if __name__ == '__main__':
    main()
