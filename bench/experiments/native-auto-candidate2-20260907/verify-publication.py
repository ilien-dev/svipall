"""Independently check saved/public bytes and accounting of the completed workload."""
from collections import Counter
import gzip
import hashlib
import json
from pathlib import Path
import zipfile

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[2]


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    manifest = read(ROOT / 'manifest.json')
    completed = read(ROOT / 'state/completed.json')
    assert len(completed) == len(manifest['schedule']) == 306
    assert [x['label'] for x in completed] == [x['label'] for x in manifest['schedule']]
    published = read(ROOT / 'verification.json')
    labels = read(ROOT / 'audit-labels.json')
    ips = read(ROOT / 'state/ip-redactions.json')
    paths = read(ROOT / 'state/paths.json')
    assert sha(Path(paths['binary']).read_bytes()) == manifest['binary_sha256']
    assert sha(Path(paths['browser']).read_bytes()) == manifest['browser_sha256']
    for name, expected in manifest['runtime_sha256'].items():
        assert sha((ROOT / 'bin' / name).read_bytes()) == expected
    for name in ['protocol', 'targets']:
        suffix = '.md' if name == 'protocol' else '.json'
        assert sha((ROOT / (name + suffix)).read_bytes()) == manifest[name + '_sha256']
    with zipfile.ZipFile(ROOT / 'source-before.zip') as archive:
        for name, expected in manifest['source_sha256'].items():
            assert sha(archive.read(name)) == expected, name

    # Independently reconstruct the documented redaction, preserving every other value.
    def sanitized(value):
        if isinstance(value, dict):
            return {key: sanitized(item) for key, item in value.items()}
        if isinstance(value, list):
            return [sanitized(item) for item in value]
        if isinstance(value, str):
            for private, public in [(str(REPO), '<repo>'), (str(Path.home()), '<home>')]:
                value = value.replace(private, public).replace(private.replace('\\', '/'), public)
            for address in sorted(ips, key=len, reverse=True):
                value = value.replace(address, '<redacted-exit-ip>')
        return value

    counts = Counter()
    fingerprints = set()
    expected_files = set()
    for entry, batch in zip(completed, manifest['schedule']):
        name = entry['label']
        raw_bytes = (ROOT / 'state' / (name + '.json')).read_bytes()
        assert sha(raw_bytes) == entry['raw_sha256'], name
        raw = json.loads(raw_bytes)
        assert raw['label'] == name and raw['arm'] == batch['arm']
        assert sorted(cell['position'] for cell in raw['cells']) == [1, 2, 3]
        filename = name + '.json.gz'
        expected_files.add(filename)
        packed = (ROOT / 'results' / filename).read_bytes()
        assert sha(packed) == published['published_sha256'][filename], name
        decoded = gzip.decompress(packed).decode('utf-8')
        assert json.loads(decoded) == sanitized(raw), name
        err = (ROOT / 'state' / (name + '-stderr.txt')).read_text(encoding='utf-8')
        public_err = (ROOT / 'results' / (name + '-stderr.txt')).read_text(encoding='utf-8')
        assert public_err == sanitized(err), name
        assert not any(address in decoded or address in public_err for address in ips)
        for cell in raw['cells']:
            response = cell['response']
            key = [cell['url'], response.get('status'), response.get('content', '')]
            fingerprint = sha(json.dumps(key, ensure_ascii=False).encode())
            assert fingerprint in labels and labels[fingerprint]['verdict'] != 'unreviewed'
            fingerprints.add(fingerprint)
            counts[(batch['arm'], batch['round'])] += 1
    assert expected_files == {path.name for path in (ROOT / 'results').glob('*.json.gz')}
    assert counts == Counter({(arm, round_number): 153 for arm in ['auto', 'native'] for round_number in [1, 2, 3]})
    result = dict(verified_calls=sum(counts.values()), verified_blocks=len(completed),
        reviewed_fingerprints=len(fingerprints), raw_and_published_hashes_match=True,
        published_content_equals_redacted_original=True, known_exit_addresses_absent=True,
        frozen_source_archive_matches=True, binary_browser_runtime_match=True,
        protocol_and_targets_match=True, exact_schedule_order=True)
    (ROOT / 'independent-verification.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
