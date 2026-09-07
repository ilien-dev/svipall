import unittest
from pathlib import Path
import subprocess
import tempfile
from unittest.mock import patch

from measure_paired import schedule, validate_result, summary, source_hashes


class MeasurementTests(unittest.TestCase):
    def test_source_snapshot_includes_new_tests_but_excludes_ignored_captures(self):
        with tempfile.TemporaryDirectory(prefix='svipall-source-snapshot-') as temporary:
            root = Path(temporary)
            sources = {
                'tracked.rs': 'pub fn tracked() {}',
                'crates/example/tests/new.rs': '#[test] fn regression() {}',
                'bench/src/paired.rs': '// measurement entry',
                'scripts/measure_paired.py': '# controller',
                'scripts/test_measure_paired.py': '# tests',
                'Cargo.lock': '# locked dependencies',
                'README.md': 'Documentation checked by quality control.',
                'crates/example/assets/index.html': '<main>Compiled asset</main>',
                'crates/example/tests/fixtures/input.json': '{"fixture": true}',
                'scripts/qc.ps1': '# quality gate',
                'skill/SKILL.md': '# Compiled CLI help',
                '.gitignore': 'ignored/\n',
            }
            for name, content in dict(sources, **{
                'ignored/capture.py': '# private diagnostic capture',
                'bench/experiments/prior/record.json': '{"output": true}',
                'bench/experiments/prior/analysis.py': '# not build source',
            }).items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content, encoding='utf-8')
            subprocess.run(['git', 'init', '--quiet'], cwd=root, check=True)
            subprocess.run(['git', 'add', 'tracked.rs'], cwd=root, check=True)
            with patch('measure_paired.REPO', root):
                hashes = source_hashes()
            self.assertEqual(set(hashes), set(sources))
            self.assertTrue(all(len(value) == 64 for value in hashes.values()))

    def test_schedule_preserves_every_target_and_alternates_the_leading_arm(self):
        rows = schedule()
        self.assertEqual(len(rows), 306)
        self.assertEqual(sum(r['repeat'] for r in rows), 918)
        self.assertEqual(len({r['label'] for r in rows}), 306)
        for i in range(0, len(rows), 2):
            pair = rows[i:i + 2]
            self.assertEqual({r['arm'] for r in pair}, {'auto', 'native'})
            self.assertEqual(pair[0]['target'], pair[1]['target'])
        orders = [{r['target']: r['arm'] for r in rows if r['round'] == n and r['order'] == 0}
                  for n in range(1, 4)]
        for target in orders[0]:
            self.assertNotEqual(orders[0][target], orders[1][target])

    def test_summary_charges_failures_and_does_not_invent_a_successful_latency(self):
        s = summary([{'delivered': True, 'secs': 2}, {'delivered': False, 'secs': 60}])
        self.assertEqual(s['seconds_per_delivery'], 62)
        self.assertIsNone(summary([{'delivered': False, 'secs': 60}])['seconds_per_delivery'])

    def test_incomplete_batches_and_wrong_identity_are_rejected(self):
        batch = schedule()[0]
        with self.assertRaises(ValueError):
            validate_result({}, batch)
        data = dict(schema=2, label=batch['label'], arm=batch['arm'], set=batch['set'],
                    target=batch['target'], repeat=3, timeout_ms=60000, ended_unix=1,
                    effective_config={'browser_identity': 'wrong'}, cells=[])
        with self.assertRaises(ValueError):
            validate_result(data, batch)


if __name__ == '__main__':
    unittest.main()
