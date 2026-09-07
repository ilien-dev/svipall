"""Verify published records and aggregates offline. Run from any working directory."""
import csv
import gzip
import json
from pathlib import Path
import statistics
import sys

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[2]
sys.path.insert(0, str(REPO / 'scripts'))
from measure_automatic import digest, load, summarize_cells, validate_result


def verify():
    manifest = load(ROOT / 'manifest.json')
    targets = load(ROOT / 'targets.json')
    verification = load(ROOT / 'verification.json')
    summary = load(ROOT / 'summary.json')
    assert verification['complete'] and verification['verified_calls'] == 459
    assert verification['completed_batches'] == 9
    for name in ['protocol', 'targets']:
        suffix = 'md' if name == 'protocol' else 'json'
        assert digest(ROOT / f'{name}.{suffix}') == manifest[f'{name}_sha256']
    machine = load(ROOT / 'machine.json')
    assert digest(REPO / 'scripts/measure_automatic.py') == machine['controller_sha256'].lower()
    assert digest(REPO / 'scripts/test_measure_automatic.py') == machine['controller_tests_sha256'].lower()
    cells = []
    records = []
    for batch in manifest['schedule']:
        name = batch['label'] + '.json.gz'
        path = ROOT / 'results' / name
        assert digest(path) == verification['published_sha256'][name]
        record = json.loads(gzip.decompress(path.read_bytes()))
        validate_result(record, batch['label'], batch['set'], targets[batch['set']])
        by_name = {t['name']: t for t in targets[batch['set']]}
        for cell in record['cells']:
            response = cell['response']
            status = response.get('status')
            content = response.get('content') or ''
            expected_strings = by_name[cell['target']]['expect']
            expected = not expected_strings or any(s.lower() in content.lower() for s in expected_strings)
            valid_status = isinstance(status, int) and not isinstance(status, bool) and 100 <= status <= 599
            delivered = bool(valid_status and 200 <= status < 400 and response.get('blocked_reason') is None
                             and content.strip() and expected)
            outcome = ('error' if not valid_status else 'delivered' if delivered else
                       'wall' if response.get('blocked_reason') is not None else 'missing_content')
            assert (cell['valid_status'], cell['expected'], cell['delivered'], cell['outcome']) == (
                valid_status, expected, delivered, outcome)
            if response.get('native_fallback'):
                assert response.get('privacy_notice'), 'Native fallback must carry its notice'
        records.append(record)
        cells.extend(record['cells'])
    assert len(cells) == 459
    assert summary['overall'] == summarize_cells(cells)
    assert summary['unique_urls'] == len({c['url'] for c in cells})
    for row in summary['rows']:
        per_round = [[c for c in record['cells'] if c['position'] == row['position']]
                     for record in records if record['set'] == row['set']]
        assert row['per_round'] == [summarize_cells(group) for group in per_round]
        counts = [sum(c['delivered'] for c in group) for group in per_round]
        assert row['median_delivered'] == statistics.median(counts)
        assert row['min_delivered'] == min(counts) and row['max_delivered'] == max(counts)
        for key, value in summarize_cells(sum(per_round, [])).items():
            assert row[key] == value, (row['set'], row['position'], key)
    with (ROOT / 'calls.csv').open(encoding='utf-8', newline='') as stream:
        rows = list(csv.DictReader(stream))
    assert len(rows) == 459
    assert len({(r['label'], r['target'], r['visit']) for r in rows}) == 459
    for row, cell in zip(rows, cells):
        assert (row['target'], row['url'], int(row['visit'])) == (
            cell['target'], cell['url'], cell['position'])
        assert float(row['seconds']) == cell['secs']
        assert row['delivered'] == str(cell['delivered'])
    with (ROOT / 'targets.csv').open(encoding='utf-8', newline='') as stream:
        rows = list(csv.DictReader(stream))
    assert len(rows) == 51 and all(int(r['calls']) == 9 for r in rows)
    assert sum(int(r['passes']) for r in rows) == summary['overall']['delivered']
    for row in rows:
        group = [c for record in records if record['set'] == row['set']
                 for c in record['cells'] if c['target'] == row['target']]
        totals = summarize_cells(group)
        assert int(row['passes']) == totals['delivered']
        assert float(row['seconds']) == totals['total_fetch_seconds']
        assert int(row['empty_attempt_logs']) == totals['refused_without_attempt']
        for key in ['native_attempted', 'native_delivered', 'timeouts']:
            assert int(row[key]) == totals[key]
    print('Verified: 9 batches, 459 calls, 51 target slots; hashes, outcomes, timings and aggregates match.')


if __name__ == '__main__':
    verify()
