"""Offline regression checks for discovery, rebuilds, and failure propagation."""

import subprocess
import unittest
from pathlib import Path
from unittest.mock import Mock

from stress_contract_builds import build_orders, contract_packages, stress_build


class StressBuildTests(unittest.TestCase):
    def test_discovers_new_cdylibs_but_not_dependencies_or_native_stubs(self):
        def package(name, kinds):
            return {"id": name, "name": name, "targets": [{"crate_types": kinds}]}

        metadata = {
            "workspace_members": ["pair", "new-receiver", "native-stub", "tests"],
            "packages": [
                package("pair", ["cdylib", "rlib"]),
                package("new-receiver", ["cdylib"]),
                package("native-stub", ["rlib"]),
                package("tests", ["bin"]),
                package("external", ["cdylib"]),
            ],
        }
        self.assertEqual(contract_packages(metadata), ["new-receiver", "pair"])

    def test_empty_discovery_fails_instead_of_silently_passing(self):
        with self.assertRaises(ValueError):
            contract_packages({"workspace_members": [], "packages": []})

    def test_seed_replays_two_complete_distinct_orders(self):
        packages = ["a", "b", "c", "d", "e"]
        for seed in range(20):
            orders = build_orders(packages, seed)
            self.assertEqual(orders, build_orders(packages, seed))
            self.assertNotEqual(*orders)
            for order in orders:
                self.assertCountEqual(order, packages)
        self.assertEqual(build_orders(["only"], 1), (["only"], ["only"]))

    def test_every_build_is_preceded_by_a_scoped_clean_in_both_passes(self):
        run = Mock()
        target = Path("isolated-target")
        stress_build(["a", "b"], 338, target, run=run)
        calls = run.call_args_list
        self.assertEqual(len(calls), 8)
        built = []
        for clean, build in zip(calls[::2], calls[1::2]):
            clean_args, build_args = clean.args[0], build.args[0]
            self.assertEqual(clean_args[:2], ["cargo", "clean"])
            self.assertEqual(build_args[:4], ["cargo", "build", "--locked", "--lib"])
            self.assertEqual(clean_args[2:], build_args[4:])
            self.assertEqual(build_args[-2:], ["--target-dir", str(target)])
            self.assertTrue(build.kwargs["check"])
            built.append(build_args[build_args.index("--package") + 1])
        self.assertCountEqual(built, ["a", "a", "b", "b"])

    def test_build_failure_stops_the_run(self):
        run = Mock(side_effect=[None, subprocess.CalledProcessError(1, "cargo build")])
        with self.assertRaises(subprocess.CalledProcessError):
            stress_build(["a", "b"], 338, Path("isolated-target"), run=run)
        self.assertEqual(run.call_count, 2)


if __name__ == "__main__":
    unittest.main()
