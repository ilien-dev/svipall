"""Reproduce observed public wrapper shapes using original synthetic content, offline."""
from pathlib import Path
import json
import re
import runpy
import sys

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[2]
sys.path.insert(0, str(REPO / 'scripts'))
from measure_automatic import digest, load, save
from measure_paired import environment

INTRO = '<p>' + ('This directory records published work and the context needed to inspect each entry. ' * 8) + '</p>'


def fixture(shape):
    records = []
    for i in range(1, 6):
        title = f'Record {i}'
        link = f'https://records.test/items/{i}'
        heading = f'<h3><a href="{link}">{title}</a></h3>'
        if shape == 'decorated-heading':
            records.append(f'<article><h2>Published record</h2><div><div><div>'
                f'<span><svg aria-hidden="true"><path d="M0 0"/></svg></span>{heading}'
                f'</div><div><a href="https://records.test/stars/{i}">Star 42</a></div></div></div>'
                '<nav><a href="https://records.test/menu">Record navigation sentinel</a></nav>'
                '<div><p>A compact toolkit for producing reproducible experimental reports.</p></div></article>')
        elif shape == 'layout-table-heading':
            records.append(f'<li><div><table role="presentation"><tbody><tr><td><div>{heading}</div>'
                f'<div><span>Institute {i}</span> <span>Remote</span></div></td></tr></tbody></table>'
                '<div><p>The role covers research software, data review and the publication of '
                'reproducible measurements for independent laboratories.</p></div></div></li>')
        else:
            records.append(f'<div><div><div><span><a href="{link}"><h3>{title}</h3><br>'
                f'<div>Institute {i}</div></a></span><div><a href="https://records.test/translate/{i}">'
                'Translate</a></div></div></div><div><p>This result explains the measurement '
                'method and the published findings, including enough context for an independent '
                'reader to interpret the evidence.</p></div></div>')
    body = ''.join(records)
    if shape == 'layout-table-heading':
        body = '<ul>' + body + '</ul>'
    return '<html><body><main>' + INTRO + body + '</main></body></html>'


def main():
    fixtures = ROOT / 'record-structure-fixtures'
    fixtures.mkdir(exist_ok=False)
    state = ROOT / 'state/record-structures-red'
    state.mkdir(exist_ok=False)
    binary = ROOT / 'bin/after.exe'
    assert digest(binary) == load(ROOT / 'local-after.json')['binary_sha256']
    browser = Path(load(ROOT.parent / 'native-auto-candidate1-20260907/state/paths.json')['browser'])
    home = ROOT / 'state/public-html/replay-home-after'
    execute = runpy.run_path(str(ROOT / 'public-html-check.py'))['execute']
    rows = []
    for shape in ['decorated-heading', 'layout-table-heading', 'block-anchor-heading']:
        html = fixture(shape)
        path = fixtures / (shape + '.html')
        path.write_text(html, encoding='utf-8')
        for full in [False, True]:
            stem = state / (shape + '-' + ('full' if full else 'main'))
            command = [str(binary), 'fetch', 'raw:', '--stdin', '--out', str(stem.with_suffix('.md'))]
            if full:
                command.append('--full')
            result = execute(command, environment(home, browser), stem, stdin=html.encode())
            content = stem.with_suffix('.md').read_text(encoding='utf-8')
            row = dict(shape=shape, full=full, **result,
                record_titles=sum(f'Record {i}' in content for i in range(1, 6)),
                item_destinations=sum(f'https://records.test/items/{i}' in content for i in range(1, 6)),
                navigation_sentinel='Record navigation sentinel' in content,
                markdown_sha256=digest(stem.with_suffix('.md')), fixture_sha256=digest(path))
            rows.append(row)
            print(json.dumps({k:row[k] for k in ['shape','full','record_titles','item_destinations','navigation_sentinel']}), flush=True)
    save(ROOT / 'record-structures-red.json', dict(binary_sha256=digest(binary), rows=rows,
        expected='Five visible titles and item destinations in each mode; navigation absent in main mode.',
        note='Frozen narrow heading prototype, before the broader behavior change. Synthetic fixtures '
             'reproduce inspected public structures, with no copied public prose or live requests.'))
    assert_results(rows)


def assert_results(rows):
    failures = []
    for row in rows:
        stem = ROOT / 'state/record-structures-red' / (row['shape'] + '-' + ('full' if row['full'] else 'main'))
        assert digest(stem.with_suffix('.md')) == row['markdown_sha256']
        if row['record_titles'] != 5 or row['item_destinations'] != 5:
            failures.append(f"{row['shape']} full={row['full']}: expected 5 titles/links, "
                f"got {row['record_titles']}/{row['item_destinations']}")
        if not row['full'] and row['navigation_sentinel']:
            failures.append(row['shape'] + ': navigation leaked')
    assert not failures, '\n'.join(failures)


if __name__ == '__main__':
    if '--assert-saved' in sys.argv:
        assert_results(load(ROOT / 'record-structures-red.json')['rows'])
    else:
        main()
