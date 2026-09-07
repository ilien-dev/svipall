"""Summarize discordant audited pairs without publishing response text or addresses."""
from collections import Counter, defaultdict
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[2]
sys.path.insert(0, str(REPO / 'scripts'))
from measure_automatic import load, save
from report_paired import read_records, apply_audit, classify_stop

_, _, _, cells = read_records(ROOT)
labels = load(ROOT / 'audit-labels.json')
pairs = defaultdict(dict)
for cell in cells:
    apply_audit(cell, labels[cell['audit_id']])
    pairs[(cell['set'], cell['target'], cell['round'], cell['position'])][cell['arm']] = cell
result = {}
for endpoint in ['audited_useful', 'useful_content_available']:
    rows = []
    for key, pair in pairs.items():
        if bool(pair['auto'][endpoint]) == bool(pair['native'][endpoint]):
            continue
        winner = 'auto' if pair['auto'][endpoint] else 'native'
        loser = 'native' if winner == 'auto' else 'auto'
        sides = {}
        for arm, cell in pair.items():
            response = cell['response']
            sides[arm] = dict(stop=classify_stop(response), seconds=cell['secs'],
                audit_verdict=cell['audit_verdict'], delivered=cell['delivered'],
                tier=response.get('tier_used'), identity=response.get('identity_used'),
                attempts=[str(a).split(':', 1)[0] for a in response.get('attempts', [])],
                native_fallback=response.get('native_fallback'), status=response.get('status'))
        rows.append(dict(set=key[0], target=key[1], round=key[2], position=key[3],
            useful_arm=winner, missing_arm=loser, arms=sides))
    result[endpoint] = dict(
        discordant=len(rows), useful_only=dict(Counter(r['useful_arm'] for r in rows)),
        missing_stops={a: dict(Counter(r['arms'][a]['stop'] for r in rows if r['missing_arm'] == a))
            for a in ['auto', 'native']}, rows=rows)
result['note'] = ('Descriptive discordances in the interrupted candidate-1 workload, not '
    'independent samples or proof that the paired arm would succeed under the other history. '
    'No endpoint, budget or saved result is changed by this analysis.')
save(ROOT / 'remaining-deficits.json', result)
for endpoint, group in result.items():
    if isinstance(group, dict):
        print(endpoint, {k: v for k, v in group.items() if k != 'rows'})
