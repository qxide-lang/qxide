# Parses every *.qasm in the given directory with qiskit.qasm3.loads; exits non-zero on the first failure.
import pathlib
import sys

import qiskit.qasm3

paths = sorted(pathlib.Path(sys.argv[1]).glob("*.qasm"))
if not paths:
    sys.exit(f"no .qasm files in {sys.argv[1]}")

failed = False
for path in paths:
    try:
        qc = qiskit.qasm3.loads(path.read_text(encoding="utf-8"))
    except Exception as e:  # report every file, then fail
        failed = True
        print(f"FAIL {path}: {type(e).__name__}: {e}")
        continue
    print(f"ok   {path}: qubits={qc.num_qubits} clbits={qc.num_clbits} ops={len(qc.data)}")

sys.exit(1 if failed else 0)
