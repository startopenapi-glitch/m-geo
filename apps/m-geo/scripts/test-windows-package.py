#!/usr/bin/env python3
"""Regression checks for the narrow Tauri NSIS bundle marker exception."""
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("package_check", Path(__file__).with_name("verify-windows-package.py"))
package_check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package_check)


class BundleMarkerTests(unittest.TestCase):
    source = b"header\0__TAURI_BUNDLE_TYPE_VAR_UNK\0body"
    nsis = b"header\0__TAURI_BUNDLE_TYPE_VAR_NSS\0body"

    def test_allows_exactly_one_official_three_byte_patch(self):
        self.assertEqual(package_check.verify_bundle_marker(self.source, self.nsis), "Only the standard UNK to NSS marker differs")

    def test_identical_binary_requires_no_exception(self):
        self.assertEqual(package_check.verify_bundle_marker(self.source, self.source), "Executable identical without normalization")

    def test_rejects_any_other_changed_byte(self):
        for position in [0, 3, len(self.nsis) - 1]:
            changed = bytearray(self.nsis)
            changed[position] ^= 1
            with self.subTest(position=position), self.assertRaises(AssertionError):
                package_check.verify_bundle_marker(self.source, bytes(changed))

    def test_rejects_repeated_source_marker(self):
        with self.assertRaises(AssertionError):
            package_check.verify_bundle_marker(self.source + self.source, self.nsis + self.source)

    def test_rejects_repeated_bundled_marker(self):
        suffix = b"__TAURI_BUNDLE_TYPE_VAR_NSS"
        with self.assertRaises(AssertionError):
            package_check.verify_bundle_marker(self.source + suffix, self.nsis + suffix)

    def test_rejects_size_change(self):
        for changed in [self.nsis + b"extra", self.nsis[:-1]]:
            with self.subTest(size=len(changed)), self.assertRaises(AssertionError):
                package_check.verify_bundle_marker(self.source, changed)


if __name__ == "__main__":
    unittest.main()
