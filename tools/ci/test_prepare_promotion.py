"""Promotion keeps fixed branch roles and never merges or publishes."""
import unittest
from unittest.mock import patch
import prepare_promotion as promotion


class PromotionTests(unittest.TestCase):
    @patch.object(promotion, "gh")
    def test_reuses_existing_pr_without_changes(self, gh):
        gh.return_value = '[{"url":"https://github.com/example/repo/pull/1","isCrossRepository":false}]'
        self.assertIn("/pull/1", promotion.prepare("example/repo"))
        gh.assert_called_once()

    @patch.object(promotion, "gh")
    def test_no_ahead_commits_does_not_create_pr(self, gh):
        gh.side_effect = ['[]', '{"ahead_by":0}']
        self.assertIn("No changes", promotion.prepare("example/repo"))
        self.assertEqual(gh.call_count, 2)

    @patch.object(promotion, "gh")
    def test_creates_promotion_with_fixed_branch_roles(self, gh):
        gh.side_effect = ['[]', '{"ahead_by":2}', 'https://github.com/example/repo/pull/2']
        self.assertIn("/pull/2", promotion.prepare("example/repo"))
        arguments = gh.call_args.args
        self.assertEqual(arguments[:2], ("pr", "create"))
        self.assertEqual(arguments[arguments.index("--base") + 1], "main")
        self.assertEqual(arguments[arguments.index("--head") + 1], "develop")

    @patch.object(promotion, "gh")
    def test_api_failure_is_not_reported_as_no_changes(self, gh):
        gh.side_effect = RuntimeError("denied")
        with self.assertRaises(RuntimeError):
            promotion.prepare("example/repo")

    @patch.object(promotion, "gh")
    def test_create_failure_is_not_reported_as_success(self, gh):
        gh.side_effect = ['[]', '{"ahead_by":1}', RuntimeError("denied")]
        with self.assertRaises(RuntimeError):
            promotion.prepare("example/repo")

    @patch.object(promotion, "gh")
    def test_fork_pr_does_not_replace_promotion(self, gh):
        gh.side_effect = [
            '[{"url":"https://github.com/example/repo/pull/9","isCrossRepository":true}]',
            '{"ahead_by":1}', 'https://github.com/example/repo/pull/2',
        ]
        self.assertIn("/pull/2", promotion.prepare("example/repo"))
        self.assertEqual(gh.call_count, 3)

    @patch.object(promotion, "gh")
    def test_same_repository_pr_is_found_after_fork(self, gh):
        gh.return_value = (
            '[{"url":"https://github.com/example/repo/pull/9","isCrossRepository":true},'
            '{"url":"https://github.com/example/repo/pull/1","isCrossRepository":false}]'
        )
        self.assertIn("/pull/1", promotion.prepare("example/repo"))
        gh.assert_called_once()


if __name__ == "__main__":
    unittest.main()
