"""Offline contracts for the automatic-mode measurement controller and accounting."""
import copy
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from measure_automatic import schedule, validate_result, summarize_cells, load, save


def fixture():
    return {
        'schema': 1, 'label': 'auto-fixture-1', 'set': 'fixture', 'repeat': 3,
        'timeout_ms': 60000, 'ended_unix': 10,
        'config_toml': 'browser_identity="auto"\nauto_native_fallback=true\n',
        'cells': [
            {'target': 'a', 'url': 'https://example.test/a', 'position': i,
             'secs': seconds, 'delivered': delivered, 'outcome': outcome,
             'response': response}
            for i, seconds, delivered, outcome, response in [
                (1, 60, False, 'error', {'blocked_reason': 'timeout', 'attempts': ['http: EXC timeout']}),
                (2, 2, True, 'delivered', {'status': 200, 'content': 'Article', 'quality': 'full',
                     'identity_used': 'native', 'native_fallback': True, 'privacy_notice': 'Exposure'}),
                (3, 0.01, False, 'error', {'wall_kind': 'cooldown', 'attempts': [],
                     'blocked_reason': 'cooldown', 'cooldown_seconds_left': 900}),
            ]
        ],
    }


class MeasurementContracts(unittest.TestCase):
    def test_permanent_progress_write_denial_is_bounded_and_preserves_previous_json(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'progress.json'
            path.write_text('{"previous": true}', encoding='utf-8')
            with patch.object(Path, 'replace', side_effect=PermissionError('Denied')) as replace, patch('measure_automatic.time.sleep') as wait:
                with self.assertRaises(PermissionError):
                    save(path, {'completed_calls': 3})
            self.assertEqual(load(path), {'previous': True})
            self.assertLessEqual(replace.call_count, 20)
            self.assertLessEqual(sum(call.args[0] for call in wait.call_args_list), 2)

    def test_progress_save_survives_temporary_reader_contention(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'progress.json'
            path.write_text('{"previous": true}', encoding='utf-8')
            replace = Path.replace
            attempts = []

            def contended(source, destination):
                attempts.append(source)
                if len(attempts) < 3:
                    self.assertEqual(load(path), {'previous': True})
                    raise PermissionError('Reader temporarily holds destination')
                return replace(source, destination)

            with patch.object(Path, 'replace', contended), patch('measure_automatic.time.sleep'):
                save(path, {'completed_calls': 3})
            self.assertEqual(load(path), {'completed_calls': 3})
            self.assertFalse(path.with_suffix('.json.tmp').exists())

    def test_schedule_contains_all_nine_batches_with_balanced_set_order(self):
        rows = schedule()
        self.assertEqual(len(rows), 9)
        self.assertEqual(len({row['label'] for row in rows}), 9)
        self.assertEqual(sum(row['targets'] * 3 for row in rows), 459)
        for set_name in ['public31', 'hard12', 'vendors8']:
            self.assertEqual([r['round'] for r in rows if r['set'] == set_name], [1, 2, 3])
            self.assertEqual({i % 3 for i, r in enumerate(rows) if r['set'] == set_name}, {0, 1, 2})

    def test_every_planned_target_and_visit_must_exist_exactly_once(self):
        data = fixture()
        targets = [{'name': 'a', 'url': 'https://example.test/a'}]
        validate_result(data, 'auto-fixture-1', 'fixture', targets)
        for mutate in [lambda d: d['cells'].pop(),
                       lambda d: d['cells'].append(copy.deepcopy(d['cells'][0])),
                       lambda d: d['cells'][0].update(url='https://wrong.test')]:
            broken = copy.deepcopy(data)
            mutate(broken)
            with self.assertRaises(ValueError):
                validate_result(broken, 'auto-fixture-1', 'fixture', targets)

    def test_wrong_policy_or_timeout_and_unfinished_runs_are_rejected(self):
        for field, value in [('config_toml', 'browser_identity="native"'),
                             ('timeout_ms', 90000), ('ended_unix', None)]:
            broken = fixture()
            broken[field] = value
            with self.assertRaises(ValueError):
                validate_result(broken, 'auto-fixture-1', 'fixture',
                                [{'name': 'a', 'url': 'https://example.test/a'}])

    def test_failures_and_local_deferrals_do_not_make_delivery_look_fast(self):
        result = summarize_cells(fixture()['cells'])
        self.assertEqual(result['calls'], 3)
        self.assertEqual(result['delivered'], 1)
        self.assertEqual(result['refused_without_attempt'], 1)
        self.assertAlmostEqual(result['total_fetch_seconds'], 62.01)
        self.assertAlmostEqual(result['fetch_seconds_per_delivery'], 62.01)
        self.assertEqual(result['delivered_median_seconds'], 2)
        self.assertEqual(result['native_attempted'], 1)
        self.assertEqual(result['native_delivered'], 1)

    def test_zero_delivery_has_no_finite_cost_per_delivery(self):
        cells = [fixture()['cells'][0]]
        result = summarize_cells(cells)
        self.assertIsNone(result['fetch_seconds_per_delivery'])
        self.assertIsNone(result['delivered_median_seconds'])


if __name__ == '__main__':
    unittest.main()
