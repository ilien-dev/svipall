"""Offline checks for the supplementary audit; no requests or rescoring."""
import unittest

from report import decorate, preview


class AuditTests(unittest.TestCase):
    def test_plain_text_with_unfinished_entity_is_flushed(self):
        self.assertEqual(preview('Jobs &amp'), 'Jobs &')

    def test_static_preview_ignores_head_and_scripts(self):
        self.assertEqual(preview('<head><title>Name</title></head><body>Useful '
                                 '<script>hidden</script>text</body>'), 'Useful text')

    def test_paged_html_is_flagged_without_changing_delivery(self):
        cell = dict(delivered=True, response=dict(
            content='<html><head><title>Name</title>', quality='thin',
            truncated=True, cursor='continue-here', attempts=['browser: EXC launching browser (5ms)']))
        result = decorate([dict(label='auto-public31-1', set='public31', cells=[cell])])[0]
        self.assertTrue(result['delivered'])
        self.assertIn('truncated_response', result['audit_flags'])
        self.assertIn('html_without_body_tag', result['audit_flags'])
        self.assertEqual(result['browser_launch_errors'], 1)

    def test_complete_plain_text_is_not_flagged_as_html(self):
        cell = dict(delivered=True, response=dict(content='Useful text. ' * 100, quality='full'))
        result = decorate([dict(label='auto-hard12-1', set='hard12', cells=[cell])])[0]
        self.assertEqual(result['audit_flags'], [])

    def test_empty_timeout_log_does_not_prove_local_refusal(self):
        cells = [dict(delivered=False, response=dict(attempts=[], blocked_reason=reason))
                 for reason in ['timeout', 'cooldown', 'address_budget']]
        results = decorate([dict(label='auto-hard12-1', set='hard12', cells=cells)])
        self.assertEqual([r['confirmed_local_deferral'] for r in results], [False, True, True])


if __name__ == '__main__':
    unittest.main()
