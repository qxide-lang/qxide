# Roadmap 4c: `import qxide` runs the CLI that the same wheel installs. Run with the wheel
# installed: python -m unittest discover -s python/tests
import os
import tempfile
import unittest

import qxide

BELL = """void main() {
    Qubit a = qubit();
    Qubit b = qubit();
    h(a);
    cx(a, b);
    println("{}{}", measure(a), measure(b));
}
"""


class QxideModule(unittest.TestCase):
    def test_binary_and_version(self):
        self.assertTrue(os.path.isfile(qxide.binary()))
        self.assertRegex(qxide.version(), r"^\d+\.\d+\.\d+")

    def test_run_source(self):
        r = qxide.run(BELL, seed=3)
        self.assertTrue(r.ok, r.stderr)
        self.assertIn(r.stdout, ("00\n", "11\n"))
        self.assertEqual(qxide.run(BELL, seed=3).stdout, r.stdout)
        many = qxide.run(BELL, shots=50, seed=1)
        self.assertTrue(many.ok)
        self.assertNotIn("01", many.stdout)

    def test_args_stdin_and_exit_code(self):
        src = "import std.io;\n\nint main(String[] args) {\n    io.writeLine(args[0] + \" \" + io.readAll().trim());\n    return 3;\n}\n"
        r = qxide.run(src, args=["hi"], stdin="there\n")
        self.assertEqual(r.returncode, 3)
        self.assertEqual(r.stdout, "hi there\n")

    def test_check_reports_diagnostics(self):
        self.assertTrue(qxide.check(BELL).ok)
        r = qxide.check("void main() {\n    Qubit q = qubit();\n}\n")
        self.assertEqual(r.returncode, 1)
        self.assertIn("Q0302", r.stderr)

    def test_test_runner(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "t.qx")
            with open(path, "w", encoding="utf-8") as f:
                f.write("import std.test;\n\nvoid testOne() {\n    test.assertTrue(1 < 2, \"order\");\n}\n")
            r = qxide.test(path)
            self.assertTrue(r.ok, r.stdout + r.stderr)
            self.assertIn("1 passed; 0 failed", r.stdout)


if __name__ == "__main__":
    unittest.main()
