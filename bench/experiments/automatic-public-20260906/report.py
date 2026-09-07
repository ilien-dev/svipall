"""Generate audit previews and final CSV/Markdown tables from recorded calls only."""
import argparse
import csv
from html.parser import HTMLParser
import hashlib
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT.parents[2] / 'scripts'))
from measure_automatic import discover_ips, load, summarize_cells, validate_result


class VisiblePreview(HTMLParser):
    """Static audit aid, not the product extractor or a new delivery-scoring rule."""
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.hidden = []
        self.pieces = []

    def handle_starttag(self, tag, attrs):
        if tag in {'script', 'style', 'head', 'noscript', 'template', 'svg'}:
            self.hidden.append(tag)

    def handle_endtag(self, tag):
        if self.hidden and tag == self.hidden[-1]:
            self.hidden.pop()

    def handle_data(self, data):
        if not self.hidden:
            self.pieces.append(data)


def preview(content):
    reader = VisiblePreview()
    reader.feed(content)
    reader.close()
    return re.sub(r'\s+', ' ', ' '.join(reader.pieces)).strip()


def records(require_complete):
    completed = load(ROOT / 'state/completed.json')
    manifest, targets = load(ROOT / 'manifest.json'), load(ROOT / 'targets.json')
    if require_complete and len(completed) != 9:
        raise ValueError('Final tables require all nine batches')
    rows = []
    for batch in completed:
        data = load(ROOT / f'state/{batch["label"]}.json')
        validate_result(data, data['label'], data['set'], targets[data['set']])
        rows.append(data)
    return rows


def decorate(rows):
    addresses = discover_ips(rows)
    cells = []
    for record in rows:
        for cell in record['cells']:
            response = cell['response']
            text = preview(str(response.get('content') or ''))
            flags = []
            content = str(response.get('content') or '')
            if cell['delivered'] and response.get('truncated'):
                flags.append('truncated_response')
            if (cell['delivered'] and re.search(r'<(?:!doctype\s+html|html)\b', content, re.I)
                    and not re.search(r'<body\b', content, re.I)):
                flags.append('html_without_body_tag')
            if cell['delivered'] and len(text) < 500:
                flags.append('short_static_text')
            if cell['delivered'] and response.get('quality') in ['thin', 'partial']:
                flags.append('quality_' + response['quality'])
            if cell['delivered'] and re.search(
                    r'access denied|something went wrong|enable javascript|page not found|checking your browser',
                    text, flags=re.I):
                flags.append('possible_error_or_shell_text')
            public_text = text
            for address in addresses:
                public_text = public_text.replace(address, '<redacted-exit-ip>')
            cells.append(dict(
                **cell, label=record['label'], set=record['set'],
                round=int(record['label'].rsplit('-', 1)[1]),
                audit_static_text_chars=len(text), audit_flags=flags,
                browser_launch_errors=sum('EXC launching browser' in str(a)
                                          for a in response.get('attempts', [])),
                confirmed_local_deferral=(not cell['delivered'] and
                    response.get('attempts') == [] and
                    (response.get('blocked_reason') in ['cooldown', 'address_budget'] or
                     (response.get('network_attempted') is False and response.get('blocked_reason')
                      in ['cooldown', 'over_budget', 'traffic_state', 'saturation']))),
                audit_text_sha256=hashlib.sha256(public_text.encode()).hexdigest(),
                audit_preview=public_text[:1800],
            ))
    return cells


def preview_report(cells):
    rows = []
    for set_name, target in sorted({(c['set'], c['target']) for c in cells}):
        group = [c for c in cells if (c['set'], c['target']) == (set_name, target)]
        passing = sorted([c for c in group if c['delivered']], key=lambda c: c['audit_static_text_chars'])
        selected = [passing[0]] if passing else [group[0]]
        if len(passing) > 1 and passing[-1]['audit_text_sha256'] != passing[0]['audit_text_sha256']:
            selected.append(passing[-1])
        rows.append(dict(set=set_name, target=target, url=group[0]['url'],
                         calls=len(group), passes=sum(c['delivered'] for c in group),
                         samples=[dict(label=c['label'], visit=c['position'], passed=c['delivered'],
                                       title=c['response'].get('title'), quality=c['response'].get('quality'),
                                       chars=c['audit_static_text_chars'], flags=c['audit_flags'],
                                       truncated=c['response'].get('truncated', False),
                                       continuation_available=bool(c['response'].get('cursor')),
                                       preview=c['audit_preview']) for c in selected]))
    (ROOT / 'state/audit-preview.json').write_text(json.dumps(rows, indent=2, ensure_ascii=False), encoding='utf-8')
    print(f'Audit previews: {len(rows)} target slots, {len(cells)} recorded calls.')


def write_csv(path, fields, rows):
    with path.open('w', newline='', encoding='utf-8') as stream:
        writer = csv.DictWriter(stream, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)


def final_tables(cells):
    summary = load(ROOT / 'summary.json')
    if summary['calls'] != 459 or len(cells) != 459:
        raise ValueError('Expected 459 calls')
    call_rows = []
    for c in cells:
        response = c['response']
        call_rows.append(dict(label=c['label'], set=c['set'], round=c['round'], target=c['target'],
            url=c['url'], visit=c['position'], delivered=c['delivered'], outcome=c['outcome'],
            seconds=c['secs'], status=response.get('status'), quality=response.get('quality'),
            identity_used=response.get('identity_used'), native_attempted=response.get('native_fallback'),
            empty_attempt_log=not c['delivered'] and response.get('attempts') == [],
            confirmed_local_deferral=c['confirmed_local_deferral'],
            wall_kind=response.get('wall_kind'), stopped_reason=response.get('stopped_reason'),
            truncated=response.get('truncated', False), continuation_available=bool(response.get('cursor')),
            browser_launch_errors=c['browser_launch_errors'],
            static_text_chars=c['audit_static_text_chars'], audit_flags=';'.join(c['audit_flags']),
            audit_text_sha256=c['audit_text_sha256']))
    write_csv(ROOT / 'calls.csv', list(call_rows[0]), call_rows)
    targets = []
    for set_name, target in sorted({(c['set'], c['target']) for c in cells}):
        group = [c for c in cells if (c['set'], c['target']) == (set_name, target)]
        totals = summarize_cells(group)
        targets.append(dict(set=set_name, target=target, url=group[0]['url'], calls=len(group),
            passes=totals['delivered'], native_attempted=totals['native_attempted'],
            native_delivered=totals['native_delivered'], empty_attempt_logs=totals['refused_without_attempt'],
            confirmed_local_deferrals=sum(c['confirmed_local_deferral'] for c in group),
            timeouts=totals['timeouts'], seconds=totals['total_fetch_seconds'],
            median_seconds=totals['median_call_seconds'],
            pass_counts_by_round='/'.join(str(sum(c['delivered'] for c in group if c['round'] == i)) for i in range(1, 4)),
            passing_calls_flagged_for_review=sum(bool(c['audit_flags']) and c['delivered'] for c in group)))
    write_csv(ROOT / 'targets.csv', list(targets[0]), targets)
    lines = ['# Measured automatic-mode results', '',
        'Three rounds with persistent state; positions are consecutive calls within each target batch.',
        'Delivery means the frozen check passed, not proven content completeness. Failures remain in every denominator.', '',
        '| Set | Position | Passes per round | Median / targets | Range | Call median / p95 (s) | Successful-call median (s) | Fetch seconds / delivery |',
        '|---|---:|---|---:|---|---:|---:|---:|']
    fmt = lambda value: '—' if value is None else f'{value:.2f}'
    for row in summary['rows']:
        per = ', '.join(str(r['delivered']) for r in row['per_round'])
        lines.append(f'| {row["set"]} | {row["position"]} | {per} | {row["median_delivered"]}/{row["targets"]} | '
                     f'{row["min_delivered"]}..{row["max_delivered"]} | '
                     f'{fmt(row["median_call_seconds"])}/{fmt(row["p95_call_seconds"])} | '
                     f'{fmt(row["delivered_median_seconds"])} | {fmt(row["fetch_seconds_per_delivery"])} |')
    lines += ['', 'The last column includes time spent on failures and local refusals, divided by delivered results.',
              'Controller pauses and process startup/shutdown overhead are included in wall-clock duration, not fetch seconds.', '',
              '| Set | Calls | Passes | Native recorded / delivered | Confirmed local deferrals | Empty attempt logs | Timeouts | Fetch seconds |',
              '|---|---:|---:|---:|---:|---:|---:|---:|']
    for set_name, _ in [('public31', 31), ('hard12', 12), ('vendors8', 8)]:
        s = summarize_cells([c for c in cells if c['set'] == set_name])
        lines.append(f'| {set_name} | {s["calls"]} | {s["delivered"]} | '
                     f'{s["native_attempted"]}/{s["native_delivered"]} | '
                     f'{sum(c["confirmed_local_deferral"] for c in cells if c["set"] == set_name)} | '
                     f'{s["refused_without_attempt"]} | '
                     f'{s["timeouts"]} | {s["total_fetch_seconds"]:.2f} |')
    lines += ['', 'The frozen summary field `refused_without_attempt` counts empty attempt logs. An outer timeout can also',
              'produce an empty log, so the separate confirmed-deferral count requires an explicit local policy reason.',
              'Native counts reflect recorded fallback flags; an outer timeout can lose internal attempt details.',
              '', f'Total recorded calls: {len(cells)}. Unique requested URLs: {summary["unique_urls"]}.',
              f'Controller wall time: {summary["controller_wall_seconds"] / 60:.2f} minutes.',
              '', 'See `calls.csv`, `targets.csv`, `summary.json`, the content audit and compressed raw records for detail.']
    (ROOT / 'results.md').write_text('\n'.join(lines) + '\n', encoding='utf-8')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--preview', action='store_true')
    args = parser.parse_args()
    cells = decorate(records(require_complete=not args.preview))
    preview_report(cells)
    if not args.preview:
        final_tables(cells)
