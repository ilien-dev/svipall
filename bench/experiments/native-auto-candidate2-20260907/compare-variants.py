"""Build descriptive final comparison only after all three workloads are complete and audited."""
from collections import Counter, defaultdict
from pathlib import Path
import json, statistics, sys

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[2]
sys.path.insert(0, str(REPO / 'scripts'))
from measure_automatic import load, save, digest
from report_paired import read_records, apply_audit, classify_stop, paired_counts

results = []
for name in ['native-auto-20260906', 'native-auto-candidate1-20260907', ROOT.name]:
    folder = ROOT.parent / name
    manifest, completed, _, cells = read_records(folder)
    labels = load(folder / 'audit-labels.json')
    assert len(cells) == manifest['calls'] == 918 and len(completed) == 306, name
    assert all(c['audit_id'] in labels for c in cells), name
    for cell in cells: apply_audit(cell, labels[cell['audit_id']])
    targets = load(folder / 'targets.json')
    diagnostic = {t['url'] for group in targets.values() for t in group
                  if t['wall'] in ['panel', 'tls'] or t['url'] == 'https://nowsecure.nl/'}
    production = [c for c in cells if c['url'] not in diagnostic]
    pairs = defaultdict(dict)
    for c in production: pairs[(c['set'], c['target'], c['round'], c['position'])][c['arm']] = c
    arms = {}
    for arm in ['auto', 'native']:
        group = [c for c in production if c['arm'] == arm]
        useful = sum(c['audited_useful'] for c in group)
        rounds = [sum(c['audited_useful'] for c in group if c['round'] == r) for r in [1, 2, 3]]
        seconds = sum(c['secs'] for c in group)
        arms[arm] = dict(calls=len(group), useful=useful, useful_rate=useful / len(group),
            available=sum(c['useful_content_available'] for c in group), seconds=seconds,
            seconds_per_useful=seconds / useful if useful else None,
            useful_by_round=rounds, round_median=statistics.median(rounds),
            round_range=[min(rounds), max(rounds)], stops=dict(Counter(classify_stop(c['response']) for c in group)))
    discordances = []
    for key, pair in pairs.items():
        assert set(pair) == {'auto', 'native'}
        if pair['auto']['audited_useful'] == pair['native']['audited_useful']: continue
        winner = 'auto' if pair['auto']['audited_useful'] else 'native'
        discordances.append(dict(set=key[0], target=key[1], round=key[2], position=key[3], useful_arm=winner,
            sides={arm: dict(status=c['response'].get('status'), verdict=c['audit_verdict'],
                stop=classify_stop(c['response']), seconds=c['secs'],
                attempts=[str(a).split(':', 1)[0] for a in c['response'].get('attempts', [])])
                for arm, c in pair.items()}))
    common = [pair for pair in pairs.values() if all(c['audited_useful'] for c in pair.values())]
    results.append(dict(experiment=name, binary_sha256=manifest['binary_sha256'],
        audit_labels_sha256=digest(folder / 'audit-labels.json'),
        total_calls=len(cells), production_calls=len(production), arms=arms,
        primary_pairs=paired_counts(production, 'audited_useful'),
        available_pairs=paired_counts(production, 'useful_content_available'),
        discordances=discordances,
        common_useful=dict(pairs=len(common), seconds={a: sum(p[a]['secs'] for p in common) for a in arms}),
        execution_minutes=sum(c['ended_unix']-c['started_unix'] for c in completed)/60,
        calendar_minutes=(completed[-1]['ended_unix']-completed[0]['started_unix'])/60,
        interruption_files=[p.name for p in folder.glob('interruption*.json')]))
current = results[-1]
for previous in results[:-1]:
    previous['current_round_median_above_previous_range'] = {
        a: current['arms'][a]['round_median'] > previous['arms'][a]['round_range'][1] for a in ['auto', 'native']}
save(ROOT / 'variant-comparison.json', dict(experiments=results,
    note='Descriptive same-protocol comparisons. Repeated visits share history; three rounds do not establish statistical equivalence. Prior interruptions and temporal/network changes prevent causal attribution to code changes. No winner or plateau is assigned automatically.'))
lines = ['# Completed native versus automatic comparisons', '',
         'Useful counts exclude diagnostic endpoints. Costs include failed and deferred production calls.', '',
         '| Version | Useful auto / native | Available content auto / native | Seconds per useful auto / native | Round useful counts auto / native |',
         '|---|---:|---:|---:|---|']
for r in results:
    a,n = r['arms']['auto'],r['arms']['native']
    lines.append(f"| {r['experiment']} | {a['useful']}/{a['calls']} / {n['useful']}/{n['calls']} | {a['available']} / {n['available']} | {a['seconds_per_useful']:.2f} / {n['seconds_per_useful']:.2f} | {a['useful_by_round']} / {n['useful_by_round']} |")
lines += ['', 'Availability includes useful excerpts from responses carrying a blocked label; it is secondary to the prespecified useful-delivery endpoint.',
          'Counts and ranges are descriptive. Neither a universal winner nor statistical equivalence follows from similar totals. Interpret the final discordances and documented interruption sensitivity before drawing conclusions.', '']
(ROOT / 'variant-comparison.md').write_text('\n'.join(lines), encoding='utf-8')
print(json.dumps({r['experiment']: {a: v['useful'] for a,v in r['arms'].items()} for r in results}))
