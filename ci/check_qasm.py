# Parses every *.qasm in the given directory with the OpenQASM 3 reference parser (`openqasm3`,
# the grammar published with the specification) and imports it with `qiskit.qasm3.loads`.
# Reports every file, then exits non-zero if any check failed.
import pathlib
import sys

import openqasm3
import qiskit.qasm3

paths = sorted(pathlib.Path(sys.argv[1]).glob("*.qasm"))
if not paths:
    sys.exit(f"no .qasm files in {sys.argv[1]}")

failed = False
for path in paths:
    src = path.read_text(encoding="utf-8")
    try:
        openqasm3.parse(src)
        qc = qiskit.qasm3.loads(src)
    except Exception as e:  # report every file, then fail
        failed = True
        print(f"FAIL {path}: {type(e).__name__}: {e}")
        continue
    print(f"ok   {path}: qubits={qc.num_qubits} clbits={qc.num_clbits} ops={len(qc.data)}")

sys.exit(1 if failed else 0)
