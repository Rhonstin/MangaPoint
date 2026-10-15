"""Tests for release.py. Run from the repository root: python -m unittest tools/test_release.py"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release  # noqa: E402


class Decide(unittest.TestCase):
    def test_feature_is_a_minor_bump(self):
        self.assertEqual(release.decide([("feat: add CBZ export (#12)", "")], "1.0.0"), "1.1.0")

    def test_fix_and_perf_are_patch_bumps(self):
        self.assertEqual(release.decide([("fix: cover flips (#3)", "")], "1.0.0"), "1.0.1")
        self.assertEqual(release.decide([("perf(table): draw fewer rows", "")], "1.4.2"), "1.4.3")

    def test_the_biggest_change_wins(self):
        commits = [("fix: a", ""), ("feat: b", ""), ("fix: c", "")]
        self.assertEqual(release.decide(commits, "1.2.3"), "1.3.0")

    def test_breaking_is_major(self):
        self.assertEqual(release.decide([("feat!: new settings file", "")], "1.2.3"), "2.0.0")
        self.assertEqual(release.decide([("fix: x", "BREAKING CHANGE: library layout")], "1.2.3"), "2.0.0")

    def test_housekeeping_alone_is_no_release(self):
        commits = [("docs: readme", ""), ("chore: bump", ""), ("ci: cache", ""), ("Merge stuff", "")]
        self.assertIsNone(release.decide(commits, "1.0.0"))
        self.assertIsNone(release.decide([], "1.0.0"))

    def test_subjects_that_are_not_conventional_are_ignored(self):
        self.assertIsNone(release.classify("Fix the thing"))
        self.assertIsNone(release.decide([("Fix the thing", "")], "1.0.0"))


class Changelog(unittest.TestCase):
    def test_section_groups_commits(self):
        commits = [("feat: a (#1)", ""), ("fix: b", ""), ("chore: c", ""), ("docs: d", "")]
        section = release.changelog_section("1.1.0", commits, "2026-10-15")
        self.assertIn("## [1.1.0] - 2026-10-15", section)
        self.assertIn("### Features\n\n- a", section)
        self.assertIn("### Fixes\n\n- b", section)
        self.assertIn("### Documentation\n\n- d", section)
        self.assertNotIn("c\n", section.replace("chore", ""))  # chores are not listed

    def test_empty_release_still_has_a_line(self):
        self.assertIn("Maintenance release.", release.changelog_section("1.0.1", [], "2026-10-15"))


if __name__ == "__main__":
    unittest.main()
