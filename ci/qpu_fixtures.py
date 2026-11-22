# Writes the Qiskit Runtime sampler results used by the `qxide run --backend` tests, encoded by
# the official client (`qiskit_ibm_runtime.json.RuntimeEncoder`), so the decoder is tested on the
# exact format the client reads. Needs `qiskit` and `qiskit-ibm-runtime`; no network access.
#
#   python ci/qpu_fixtures.py tests/data/qpu
import json
import pathlib
import sys

from qiskit.primitives import PrimitiveResult
from qiskit.primitives.containers import BitArray, DataBin
from qiskit.primitives.containers.sampler_pub_result import SamplerPubResult
from qiskit_ibm_runtime.json import RuntimeDecoder, RuntimeEncoder

# name -> (register width, shots as integers with c[0] as the lowest bit)
FIXTURES = {
    "results_10bit.json": (10, [0x001, 0x001, 0x200, 0x3FF, 0x001]),
    "results_bell.json": (2, [0b00, 0b11, 0b11, 0b00, 0b11, 0b11, 0b01]),
}


def encode(num_bits, shots):
    bits = BitArray.from_samples(shots, num_bits)
    result = PrimitiveResult(
        [SamplerPubResult(DataBin(c=bits, shape=()), metadata={"circuit_metadata": {}})],
        metadata={"version": 2},
    )
    text = json.dumps(result, cls=RuntimeEncoder)
    decoded = json.loads(text, cls=RuntimeDecoder)[0].data.c
    assert decoded.get_int_counts() == bits.get_int_counts()
    return text


out = pathlib.Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
for name, (num_bits, shots) in FIXTURES.items():
    (out / name).write_text(encode(num_bits, shots) + "\n", encoding="utf-8")
    print(out / name)
