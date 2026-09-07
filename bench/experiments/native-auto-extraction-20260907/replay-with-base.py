"""Offline same-document replays with captured relative URLs resolved explicitly."""
from html import escape, unescape
from html.parser import HTMLParser
from pathlib import Path
import json
import re
import runpy
import sys
from urllib.parse import urljoin, urlsplit

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[2]
sys.path.insert(0, str(REPO / 'scripts'))
from measure_automatic import digest, load, save
from measure_paired import environment

ATTR = re.compile(r'''(?P<lead>\s(?:href|src|data-src)\s*=\s*)(?P<value>"[^"]*"|'[^']*'|[^\s>]+)''', re.I)


def resolve_attributes(html, base):
    # The CLI's raw: input has no URL origin. Only replace URL attributes, preserving every
    # other source byte, including script/style/text and structural whitespace.
    class Tags(HTMLParser):
        def __init__(self):
            super().__init__(convert_charrefs=False)
            self.tags = []

        def handle_starttag(self, tag, attrs):
            self.tags.append((self.getpos(), tag, attrs, self.get_starttag_text()))

        handle_startendtag = handle_starttag

    parser = Tags()
    parser.feed(html)
    for _, tag, attrs, _ in parser.tags:
        if tag == 'base' and dict(attrs).get('href'):
            proposed = urljoin(base, dict(attrs)['href'])
            if urlsplit(proposed).scheme in ['http', 'https']:
                base = proposed
            break
    lines = [0]
    lines.extend(m.end() for m in re.finditer('\n', html))
    replacements = []
    for (line, column), _, _, raw in parser.tags:
        start = lines[line-1] + column

        def replace(match):
            quoted = match['value']
            quote = quoted[0] if quoted[0] in ['"', "'"] else '"'
            value = unescape(quoted[1:-1] if quoted[0] in ['"', "'"] else quoted).strip()
            if not value or value.startswith('#') or urlsplit(value).scheme:
                return match[0]
            absolute = urljoin(base, value)
            if urlsplit(absolute).scheme not in ['http', 'https']:
                return match[0]
            return match['lead'] + quote + escape(absolute, quote=True) + quote

        rewritten = ATTR.sub(replace, raw)
        if rewritten != raw:
            assert html[start:start+len(raw)] == raw
            replacements.append((start, start+len(raw), rewritten))
    for start, end, value in reversed(replacements):
        html = html[:start] + value + html[end:]
    return html, len(replacements)


def main():
    fixture = '<p><a href="/item?q=1&amp;x=2">Title</a></p><script>"<a href=\'/skip\'>"</script><a href="#part">Part</a>'
    rewritten, count = resolve_attributes(fixture, 'https://records.test/search')
    assert count == 1
    assert rewritten == fixture.replace('href="/item?', 'href="https://records.test/item?')
    state = ROOT / 'state/public-html'
    assert load(state / 'progress.json')['state'] == 'complete'
    output_root = state / 'resolved-replays'
    output_root.mkdir(exist_ok=False)
    execute = runpy.run_path(str(ROOT / 'public-html-check.py'))['execute']
    browser = Path(load(ROOT.parent / 'native-auto-candidate1-20260907/state/paths.json')['browser'])
    rows = []
    for name in ['github-explore', 'indeed-jobs', 'google-search']:
        metadata = load(state / (name + '-capture.json'))
        base = metadata.get('final_url') or metadata.get('url')
        assert urlsplit(base).scheme in ['http', 'https']
        html, changed = resolve_attributes((state / (name + '.html')).read_text(encoding='utf-8'), base)
        source = output_root / (name + '.html')
        source.write_text(html, encoding='utf-8')
        row = dict(target=name, source_capture_sha256=digest(state / (name + '.html')),
            replay_html_sha256=digest(source), tags_with_resolved_urls=changed, replays=[])
        for arm in ['before', 'after']:
            binary = ROOT / f'bin/{arm}.exe'
            assert digest(binary) == load(ROOT / f'local-{arm}.json')['binary_sha256']
            home = state / ('replay-home-' + arm)
            for full in [False, True]:
                stem = output_root / (name + '-' + arm + '-' + ('full' if full else 'main'))
                output = stem.with_suffix('.md')
                command = [str(binary), 'fetch', 'raw:', '--stdin', '--out', str(output)]
                if full:
                    command.append('--full')
                result = execute(command, environment(home, browser), stem, stdin=html.encode())
                content = output.read_text(encoding='utf-8') if output.exists() else ''
                row['replays'].append(dict(arm=arm, full=full, **result,
                    markdown_sha256=digest(output) if output.exists() else None,
                    characters=len(content), headings=len(re.findall(r'^#{1,6} ', content, re.M)),
                    links=len(re.findall(r'\]\(', content))))
        full = [r['markdown_sha256'] for r in row['replays'] if r['full']]
        row['full_outputs_equal'] = len(full) == 2 and full[0] is not None and full[0] == full[1]
        rows.append(row)
        print(json.dumps(dict(target=name, resolved_tags=changed,
            counts=[{k:r[k] for k in ['arm','full','characters','headings','links']} for r in row['replays']],
            full_outputs_equal=row['full_outputs_equal'])), flush=True)
    save(ROOT / 'public-html-resolved-replay.json', dict(calls=rows,
        note='Offline only: resolve relative href/src/data-src against the captured response URL '
             'because raw: has no origin. All other HTML source bytes are preserved. Both frozen '
             'extractors receive identical derived HTML. Initial unnormalized replays are retained '
             'but their link counts do not represent extraction with the original page origin.'))


if __name__ == '__main__':
    main()
