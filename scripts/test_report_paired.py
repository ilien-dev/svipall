import unittest
from report_paired import audit_id, paired_counts, classify_stop, exclude_interrupted_target, apply_audit, without_deferred_pairs, round_break_sensitivity


class ReportTests(unittest.TestCase):
    def test_round_break_keeps_resumed_results_separate_without_losing_pairs(self):
        cells = [dict(set='fixture', target='one', round=r, arm=a, position=1,
                      delivered=r == 3, audited_useful=r == 3, useful_content_available=r == 3,
                      secs=2, response={})
                 for r in [1, 2, 3] for a in ['auto', 'native']]
        result = round_break_sensitivity(cells, 3)
        self.assertEqual(result['before']['calls'], 4)
        self.assertEqual(result['after']['calls'], 2)
        self.assertEqual(result['before']['primary_pairs']['neither'], 2)
        self.assertEqual(result['after']['primary_pairs']['both'], 1)
        self.assertEqual(result['before']['arms']['auto']['count'], 0)
        self.assertEqual(result['after']['arms']['auto']['count'], 1)
        self.assertEqual(result['before']['arms']['auto']['seconds'], 4)
        self.assertEqual(len(cells), 6)

    def test_deferral_sensitivity_removes_both_members_and_keeps_network_failures(self):
        base = dict(set='fixture', target='one', round=1)
        rows = [dict(base, position=1, arm='auto', response=dict(attempts=['http: 200']))]
        rows += [dict(base, position=1, arm='native', response=dict(attempts=[], blocked_reason='cooldown'))]
        rows += [dict(base, position=2, arm=a, response=dict(attempts=['warm: timeout'], blocked_reason='timeout')) for a in ['auto', 'native']]
        self.assertEqual(without_deferred_pairs(rows), rows[2:])

    def test_useful_content_on_a_blocked_response_remains_visible_separately(self):
        cell = dict(delivered=False)
        apply_audit(cell, dict(verdict='useful'))
        self.assertFalse(cell['audited_useful'])
        self.assertTrue(cell['useful_content_available'])
        apply_audit(cell, dict(verdict='partial'))
        self.assertFalse(cell['useful_content_available'])

    def test_audit_fingerprints_do_not_reveal_or_depend_on_the_arm(self):
        c = dict(url='https://example.test/article', response=dict(status=200, content='Article text'))
        self.assertEqual(audit_id(dict(c, arm='auto')), audit_id(dict(c, arm='native')))
        self.assertNotEqual(audit_id(c), audit_id(dict(c, response=dict(status=200, content='Login'))))

    def test_pair_comparison_requires_both_arms_and_retains_failures(self):
        base = dict(set='fixture', target='one', round=1, position=1)
        rows = [dict(base, arm='auto', delivered=True), dict(base, arm='native', delivered=False)]
        self.assertEqual(paired_counts(rows), dict(pairs=1, both=0, auto_only=1, native_only=0, neither=0, incomplete=0))
        self.assertEqual(paired_counts(rows[:1])['incomplete'], 1)
        self.assertEqual(paired_counts(rows[:1])['auto_only'], 0)

    def test_an_empty_timeout_log_is_not_a_confirmed_local_deferral(self):
        self.assertEqual(classify_stop(dict(blocked_reason='timeout', attempts=[])), 'timeout')
        self.assertEqual(classify_stop(dict(blocked_reason='address_budget', attempts=[])), 'local_deferral')

    def test_shutdown_sensitivity_excludes_the_affected_target_in_every_round(self):
        cells = [dict(set='public31', target='affected', round=r, arm=a, position=1, delivered=True)
                 for r in range(1, 4) for a in ['auto', 'native']]
        cells += [dict(set='hard12', target='control', round=1, arm=a, position=1, delivered=False)
                  for a in ['auto', 'native']]
        retained = exclude_interrupted_target(cells, dict(set='public31', target='affected'))
        self.assertEqual(len(retained), 2)
        self.assertEqual(len(cells), 8)
        self.assertEqual(paired_counts(retained)['neither'], 1)


if __name__ == '__main__':
    unittest.main()
