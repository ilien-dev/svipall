"""Offline comparison tables and arm-blinded content audit inputs from preserved calls."""
import argparse
from collections import Counter, defaultdict
import csv
import gzip
import hashlib
import json
from pathlib import Path
import statistics

from measure_automatic import REPO, digest, discover_ips, load, save
from measure_paired import summary, validate_result


def audit_id(cell):
    response = cell['response']
    key = [cell['url'], response.get('status'), response.get('content', '')]
    return hashlib.sha256(json.dumps(key, ensure_ascii=False).encode()).hexdigest()


def classify_stop(response):
    reason = response.get('blocked_reason')
    if response.get('attempts') == [] and reason in ['cooldown', 'address_budget']:
        return 'local_deferral'
    if 'timeout' in str(reason).lower() or 'timeout' in str(response.get('stopped_reason', '')).lower():
        return 'timeout'
    if reason:
        return 'wall_or_error'
    return 'response'


def exclude_interrupted_target(cells, affected):
    return [c for c in cells if (c['set'], c['target']) != (affected['set'], affected['target'])]


def apply_audit(cell, label):
    cell['audit_verdict'] = label.get('verdict', 'unreviewed')
    cell['useful_content_available'] = label.get('verdict') == 'useful'
    cell['audited_useful'] = cell['delivered'] and cell['useful_content_available']


def endpoint_summary(cells, endpoint):
    count = sum(bool(c[endpoint]) for c in cells)
    seconds = sum(c['secs'] for c in cells)
    return dict(calls=len(cells), count=count, seconds=seconds,
                seconds_per_result=seconds / count if count else None)


def without_deferred_pairs(cells):
    groups = defaultdict(list)
    for c in cells:
        groups[(c['set'], c['target'], c['round'], c['position'])].append(c)
    return [c for pair in groups.values()
            if {c['arm'] for c in pair} == {'auto', 'native'}
            and all(classify_stop(c['response']) != 'local_deferral' for c in pair)
            for c in pair]


def paired_counts(cells, endpoint='delivered'):
    groups = defaultdict(dict)
    for c in cells:
        key = (c['set'], c['target'], c['round'], c['position'])
        if c['arm'] in groups[key]:
            raise ValueError('Duplicate arm in matched pair')
        groups[key][c['arm']] = bool(c.get(endpoint))
    out = dict(pairs=0, both=0, auto_only=0, native_only=0, neither=0, incomplete=0)
    for arms in groups.values():
        if set(arms) != {'auto', 'native'}:
            out['incomplete'] += 1
            continue
        out['pairs'] += 1
        key = 'both' if all(arms.values()) else 'auto_only' if arms['auto'] else 'native_only' if arms['native'] else 'neither'
        out[key] += 1
    return out


def round_break_sensitivity(cells, first_resumed_round):
    result = {}
    for name, group in [('before', [c for c in cells if c['round'] < first_resumed_round]),
                        ('after', [c for c in cells if c['round'] >= first_resumed_round])]:
        result[name] = dict(calls=len(group),
            primary_pairs=paired_counts(group, 'audited_useful'),
            delivery_pairs=paired_counts(group),
            available_content_pairs=paired_counts(group, 'useful_content_available'),
            arms={a: endpoint_summary([c for c in group if c['arm'] == a], 'audited_useful')
                  for a in ['auto', 'native']})
    return result


def read_records(root):
    manifest = load(root / 'manifest.json')
    completed_path = root / 'state/completed.json'
    completed = load(completed_path) if completed_path.exists() else []
    batches = {b['label']: b for b in manifest['schedule']}
    if len({r['label'] for r in completed}) != len(completed):
        raise ValueError('Duplicate completed batch')
    records, cells = [], []
    for entry in completed:
        path = root / f'state/{entry["label"]}.json'
        if digest(path) != entry['raw_sha256']:
            raise ValueError('Recorded response changed')
        data = load(path)
        batch = batches[entry['label']]
        validate_result(data, batch)
        records.append(data)
        for cell in data['cells']:
            cells.append(dict(**cell, arm=batch['arm'], set=batch['set'], round=batch['round'],
                              order=batch['order'], label=batch['label'], audit_id=audit_id(cell)))
    return manifest, completed, records, cells


def write_csv(path, rows):
    if not rows:
        return
    with path.open('w', newline='', encoding='utf-8') as stream:
        writer = csv.DictWriter(stream, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)


def audit(root, cells):
    # No identity, tier, attempts or timing enters the blind review input. One entry per exact
    # content fingerprint, regardless of how many arms, rounds or repeated calls returned it.
    distinct = {}
    for cell in cells:
        response = cell['response']
        distinct.setdefault(cell['audit_id'], dict(id=cell['audit_id'], url=cell['url'],
            target=cell['target'], status=response.get('status'), title=response.get('title'),
            content=response.get('content', '')))
    destination = root / 'state/audit-blind'
    destination.mkdir(exist_ok=True)
    for key, entry in distinct.items():
        save(destination / f'{key}.json', entry)
    save(root / 'state/audit-index.json', [dict(id=k, target=v['target'], url=v['url'],
         status=v['status'], chars=len(v['content'])) for k, v in sorted(distinct.items())])
    return len(distinct)


def report(root, publish=False):
    manifest, completed, records, cells = read_records(root)
    complete = len(completed) == len(manifest['schedule']) and len(cells) == manifest['calls']
    if publish and not complete:
        raise ValueError('Publication requires the complete frozen workload')
    n_audit = audit(root, cells)
    audit_path = root / 'audit-labels.json'
    labels = load(audit_path) if audit_path.exists() else {}
    allowed = {'useful', 'partial', 'shell', 'wall', 'unavailable', 'diagnostic-only', 'uncertain'}
    for label in labels.values():
        if label.get('verdict') not in allowed or not label.get('reason'):
            raise ValueError('Audit labels require a known verdict and an evidence-based reason')
    for cell in cells:
        label = labels.get(cell['audit_id'], {})
        apply_audit(cell, label)
    per_arm = {}
    for arm in ['auto', 'native']:
        group = [c for c in cells if c['arm'] == arm]
        per_round = {str(r): summary([c for c in group if c['round'] == r]) for r in range(1, 4)}
        useful = sum(c['audited_useful'] for c in group)
        per_arm[arm] = dict(**summary(group), per_round=per_round, audited_useful=useful,
            useful_content_available=sum(c['useful_content_available'] for c in group),
            audited_useful_by_round={str(r): endpoint_summary([c for c in group if c['round'] == r], 'audited_useful') for r in range(1, 4)},
            useful_content_by_round={str(r): endpoint_summary([c for c in group if c['round'] == r], 'useful_content_available') for r in range(1, 4)},
            by_order={str(o): endpoint_summary([c for c in group if c['order'] == o], 'audited_useful') for o in [0, 1]},
            seconds_per_audited_useful=sum(c['secs'] for c in group) / useful if useful else None,
            audit_verdicts=dict(Counter(c['audit_verdict'] for c in group)),
            stop_types=dict(Counter(classify_stop(c['response']) for c in group)),
            native_attempts=sum(c['response'].get('native_fallback') is True for c in group),
            incomplete_content=sum(c['content_complete'] is not True for c in group))
    totals = dict(complete=complete, calls=len(cells), expected_calls=manifest['calls'], arms=per_arm,
        delivery_pairs=paired_counts(cells), audited_useful_pairs=paired_counts(cells, 'audited_useful'),
        useful_content_pairs=paired_counts(cells, 'useful_content_available'),
        content_fingerprints=n_audit, labeled_fingerprints=sum(k in labels for k in {c['audit_id'] for c in cells}),
        controller_seconds=completed[-1]['ended_unix'] - completed[0]['started_unix'] if completed else 0)
    targets = load(root / 'targets.json')
    diagnostic_urls = {t['url'] for group in targets.values() for t in group
                       if t['wall'] in ['panel', 'tls'] or t['url'] == 'https://nowsecure.nl/'}
    production = [c for c in cells if c['url'] not in diagnostic_urls]
    retained = without_deferred_pairs(production)
    totals['production'] = dict(calls=len(production),
        arms={a: endpoint_summary([c for c in production if c['arm'] == a], 'audited_useful') for a in ['auto', 'native']},
        without_confirmed_local_deferrals=dict(calls=len(retained),
            primary_pairs=paired_counts(retained, 'audited_useful'),
            available_content_pairs=paired_counts(retained, 'useful_content_available')))
    recovery_path = root / 'interruption-recovery.json'
    if recovery_path.exists():
        recovery = load(recovery_path)
        affected = next(b for b in manifest['schedule'] if b['label'] == recovery['interrupted_batch'])
        retained = exclude_interrupted_target(cells, affected)
        totals['interruption'] = dict(reason=recovery['reason'], affected_set=affected['set'],
            affected_target=affected['target'], logged_completed_calls_without_response=recovery['logged_completed_calls_without_response'],
            additional_admissions_with_unknown_outcome=recovery['additional_admissions_with_unknown_outcome'],
            excluded_calls_for_sensitivity=len(cells) - len(retained),
            sensitivity_delivery_pairs=paired_counts(retained),
            sensitivity_audited_useful_pairs=paired_counts(retained, 'audited_useful'),
            sensitivity_arms={arm: summary([c for c in retained if c['arm'] == arm]) for arm in ['auto', 'native']})
    round_break_path = root / 'interruption-between-rounds.json'
    if round_break_path.exists():
        recovery = load(round_break_path)
        resumed = next(b for b in manifest['schedule'] if b['label'] == recovery['next_batch'])
        totals['round_break'] = dict(first_resumed_round=resumed['round'],
            pause_elapsed_seconds=recovery['pause_elapsed_seconds'],
            all_calls=round_break_sensitivity(cells, resumed['round']),
            production=round_break_sensitivity(production, resumed['round']))
    save(root / 'summary.json', totals)
    rows = []
    for c in cells:
        response = c['response']
        rows.append(dict(label=c['label'], set=c['set'], target=c['target'], round=c['round'],
            arm=c['arm'], order=c['order'], position=c['position'], delivered=c['delivered'],
            seconds=c['secs'], first_response_seconds=c['first_response_secs'], status=response.get('status'),
            quality=response.get('quality'), tier=response.get('tier_used'), identity=response.get('identity_used'),
            native_fallback=response.get('native_fallback', False), stop_type=classify_stop(response),
            content_complete=c['content_complete'], chunks=len(c['chunks']), audit_id=c['audit_id'],
            audit_verdict=c['audit_verdict'], audited_useful=c['audited_useful'],
            useful_content_available=c['useful_content_available']))
    write_csv(root / 'calls.csv', rows)
    fmt = lambda n: '-' if n is None else f'{n:.2f}'
    lines = ['# Native versus automatic: recorded results', '',
        f'Status: {"complete measurement" if complete else "INCOMPLETE measurement"}; {len(cells)}/{manifest["calls"]} calls.', '',
        'Delivery is the mechanical check. Useful content requires a recorded audit reason.',
        'No winner is established while the content audit or matched rounds are incomplete.', '',
        '| Arm | Calls | Delivery checks passed | Audited useful | Fetch seconds / delivery | Successful median (s) |',
        '|---|---:|---:|---:|---:|---:|']
    for arm, a in per_arm.items():
        lines.append(f'| {arm} | {a["calls"]} | {a["delivered"]} | {a["audited_useful"]} | '
            f'{fmt(a["seconds_per_delivery"])} | {fmt(a["successful_median_seconds"])} |')
    lines += ['', f'Content audit: {totals["labeled_fingerprints"]}/{n_audit} exact content fingerprints labeled.',
              '', 'All failures remain in the denominators. See calls.csv and summary.json for matched outcomes.', '']
    lines += ['The prespecified primary useful endpoint also requires mechanical delivery. A separate',
              'secondary count retains useful records present on responses labeled blocked by the tool:',
              f'auto {per_arm["auto"]["useful_content_available"]}; native {per_arm["native"]["useful_content_available"]}.',
              'This does not imply those responses are complete or unblocked. The secondary endpoint was',
              'added after review found real job records inside blocked responses; it does not replace',
              'the prespecified primary endpoint. Counts remain provisional until the audit is complete.', '']
    lines += ['Diagnostic panels, TLS endpoints and the challenge-demo landing are excluded from the',
              f'production-content cohort ({len(production)} calls). summary.json reports that cohort and',
              'a sensitivity analysis excluding both calls of any pair with a confirmed local deferral.',
              'The latter is conditional on admission history, not a randomized causal estimate.', '']
    if 'interruption' in totals:
        lines += ['An operator-reported computer shutdown interrupted one block. Its partial evidence is archived;',
            'the block is repeated with recovered traffic accounting and retained profiles. One additional',
            'completed call has no saved response, and one additional admission has an unknown outcome.',
            'These are reported separately, not scored as product failures. summary.json also compares the',
            'arms with the affected target excluded across all rounds. See interruption-recovery.json.', '']
    if 'round_break' in totals:
        pause = totals['round_break']
        lines += [f'A computer shutdown extended the pause before round {pause["first_resumed_round"]} to '
                  f'{pause["pause_elapsed_seconds"] / 3600:.2f} hours, exceeding the planned 120 seconds.',
                  'Saved calls, profiles and traffic accounting were retained. The extended pause allows',
                  'reputation and cooldown state to decay. summary.json separates results before and after',
                  'the interruption for all calls and the production cohort. Completing the workload does',
                  'not constitute an uninterrupted confirmation. See interruption-between-rounds.json.', '']
    (root / 'results.md').write_text('\n'.join(lines), encoding='utf-8')
    if publish:
        ips = discover_ips(records)
        prior = REPO / 'bench/experiments/automatic-public-20260906/state/ip-redactions.json'
        if prior.exists():
            ips.update(load(prior))
        save(root / 'state/ip-redactions.json', sorted(ips))
        def scrub(value):
            if isinstance(value, dict):
                return {k: scrub(v) for k, v in value.items()}
            if isinstance(value, list):
                return [scrub(v) for v in value]
            if isinstance(value, str):
                for private, public in [(str(REPO), '<repo>'), (str(Path.home()), '<home>')]:
                    value = value.replace(private, public).replace(private.replace('\\', '/'), public)
                for address in sorted(ips, key=len, reverse=True):
                    value = value.replace(address, '<redacted-exit-ip>')
            return value
        hashes = {}
        for record in records:
            name = record['label']
            path = root / f'results/{name}.json.gz'
            path.write_bytes(gzip.compress(json.dumps(scrub(record), ensure_ascii=False).encode(), mtime=0))
            hashes[path.name] = digest(path)
            err = root / f'state/{name}-stderr.txt'
            (root / f'results/{name}-stderr.txt').write_text(scrub(err.read_text(encoding='utf-8')), encoding='utf-8')
        save(root / 'verification.json', dict(complete=True, verified_calls=len(cells), raw_hashes_verified=True,
            published_sha256=hashes, protocol_matches=digest(root / 'protocol.md') == manifest['protocol_sha256'],
            targets_match=digest(root / 'targets.json') == manifest['targets_sha256']))
    print(json.dumps({k: totals[k] for k in ['complete', 'calls', 'delivery_pairs', 'content_fingerprints']}))
    return totals


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--publish', action='store_true')
    args = parser.parse_args()
    report(args.root.resolve(), args.publish)
