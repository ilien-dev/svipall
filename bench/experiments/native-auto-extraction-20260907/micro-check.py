"""Three alternating before/after local CPU measurements after QC has stopped."""
from pathlib import Path
import re
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[2]
CANDIDATE = ROOT.parent / 'native-auto-candidate2-20260907'
PREVIOUS = ROOT.parent / 'native-auto-candidate1-20260907'
sys.path.insert(0, str(REPO / 'scripts'))
from measure_automatic import digest, load, save
from measure_paired import environment


def main():
    qc = load(CANDIDATE / 'qc-execution.json')
    assert qc['exit_code'] == 0 and qc['source_stable'] and qc['corpus_inputs_stable']
    binaries = dict(before=PREVIOUS / 'bin/baseline.exe',
        after=CANDIDATE / 'state/compiled/svipall-bench.exe')
    assert digest(binaries['before']) == load(PREVIOUS / 'manifest.json')['binary_sha256']
    assert digest(binaries['after']) == qc['binary_sha256']
    browser = Path(load(PREVIOUS / 'state/paths.json')['browser'])
    state = ROOT / 'state/micro-check'
    state.mkdir(exist_ok=False)
    home = state / 'home'
    home.mkdir()
    rows = []
    for repeat in range(1, 4):
        for arm in (['before', 'after'] if repeat % 2 else ['after', 'before']):
            started = time.time()
            result = subprocess.run([str(binaries[arm]), 'micro', '--assert'], cwd=REPO,
                env=environment(home, browser), stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                creationflags=subprocess.CREATE_NO_WINDOW, timeout=120)
            log = result.stderr.decode('utf-8', errors='replace')
            (ROOT / f'micro-{arm}-{repeat}.txt').write_text(log, encoding='utf-8')
            measures = {}
            for match in re.finditer(r'^(?:ok|OVER)\s+(.+?)\s+([\d.]+)(ns|µs|us|ms|s)\s+\(budget', log, re.M):
                name, value, unit = match.groups()
                measures[name] = float(value) * {'ns': 0.001, 'µs': 1, 'us': 1, 'ms': 1000, 's': 1000000}[unit]
            rows.append(dict(arm=arm, repeat=repeat, exit_code=result.returncode,
                seconds=time.time()-started, microseconds=measures,
                log_sha256=digest(ROOT / f'micro-{arm}-{repeat}.txt')))
            print(f'{arm} repeat {repeat}: exit {result.returncode}, {len(measures)} measurements', flush=True)
    names = sorted(set.intersection(*(set(r['microseconds']) for r in rows)))
    compared = {}
    for name in names:
        values = {a: [r['microseconds'][name] for r in rows if r['arm'] == a] for a in binaries}
        medians = {a: statistics.median(v) for a, v in values.items()}
        ranges = {a: [min(v), max(v)] for a, v in values.items()}
        compared[name] = dict(median_microseconds=medians, range_microseconds=ranges,
            after_median_below_before_range=medians['after'] < ranges['before'][0],
            after_median_above_before_range=medians['after'] > ranges['before'][1])
    save(ROOT / 'micro-comparison.json', dict(
        binaries={a: digest(b) for a, b in binaries.items()}, rows=rows, comparisons=compared,
        note='Three sequential invocations per binary with alternating order; all observations retained. '
             'Different revisions include routing and MCP changes as well as the heading patch. '
             'These are fixture timings on this host, not isolated causal effects or public latency.'))
    assert all(r['exit_code'] == 0 for r in rows), 'A CPU/structural gate failed; all logs retained'
    assert names, 'No timing rows parsed; review raw logs'


if __name__ == '__main__':
    main()
