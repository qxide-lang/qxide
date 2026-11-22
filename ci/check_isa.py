# Checks the device circuits of `qxide run --backend ibm:<device>` against real IBM device
# definitions without any network access: for each offline snapshot in
# `qiskit_ibm_runtime.fake_provider`, writes its configuration, lets `qxide run --dry-run
# --device-config` build the circuit for every given .qx program, and requires that the circuit
# parses with the OpenQASM 3 reference parser, imports with `qiskit.qasm3.loads`, and passes
# the official client's own ISA check (`validate_isa_circuits`) for that device's target.
#
#   python ci/check_isa.py <qxide binary> <out dir> examples/qx/*.qx
import json
import pathlib
import subprocess
import sys

import openqasm3
import qiskit.qasm3
from qiskit_ibm_runtime.fake_provider import FakeBrisbane, FakeFez, FakeTorino
from qiskit_ibm_runtime.utils import validate_isa_circuits

qxide, out = sys.argv[1], pathlib.Path(sys.argv[2])
programs = sys.argv[3:]
out.mkdir(parents=True, exist_ok=True)

failed = 0
for fake in [FakeTorino(), FakeFez(), FakeBrisbane()]:
    config = out / f"{fake.name}.json"
    config.write_text(json.dumps(fake.configuration().to_dict(), default=str), encoding="utf-8")
    for program in programs:
        run = subprocess.run(
            [qxide, "run", program, "--backend", f"ibm:{fake.name}", "--shots", "10",
             "--dry-run", "--device-config", str(config)],
            capture_output=True, text=True, encoding="utf-8",
        )
        name = f"{fake.name} {pathlib.Path(program).name}"
        if run.returncode != 0:
            # Every given program measures something, so a refusal is a failure.
            failed += 1
            print(f"FAIL {name}: exit {run.returncode}: {run.stderr.strip()}")
            continue
        isa = run.stdout.split("\nOpenQASM 3:\n", 1)[1]
        try:
            openqasm3.parse(isa)
            circuit = qiskit.qasm3.loads(isa)
            validate_isa_circuits([circuit], fake.target)
        except Exception as e:  # report every pair, then fail
            failed += 1
            print(f"FAIL {name}: {type(e).__name__}: {e}")
            continue
        ops = circuit.count_ops()
        print(f"ok   {name}: qubits={circuit.num_qubits} {dict(ops)}")

sys.exit(1 if failed else 0)
