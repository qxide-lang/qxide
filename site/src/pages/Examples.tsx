import {Link} from 'react-router';
import {CodeBlock} from '../components/CodeBlock';
import {GITHUB_BLOB, GITHUB_TREE, usePageMeta} from '../lib/meta';

type Example = {
  name: string;
  summary: string;
  command: string;
  result?: {label: string; text: string; note: string};
  note?: string;
};

// .qx programs in examples/qx/ at v0.8.0. Results were captured by running the commands with qxide 0.8.0.
const QX_EXAMPLES: Example[] = [
  {
    name: 'bell',
    summary: 'A Bell pair: the two measured bits are always equal.',
    command: 'qxide run examples/qx/bell.qx --shots 1000 --seed 0',
    result: {
      label: 'Output',
      text: '[1, 1]  0b11 = 3  ×521\n  a=1 b=1\n[0, 0]  0b00 = 0  ×479\n  a=0 b=0',
      note: '[0, 1] and [1, 0] never occur because the qubits are entangled.',
    },
  },
  {
    name: 'ghz',
    summary: 'A 5-qubit GHZ state: the register always reads all zeros or all ones.',
    command: 'qxide run examples/qx/ghz.qx --shots 1000 --seed 0',
    result: {
      label: 'Output',
      text: '[1, 1, 1, 1, 1]  0b11111 = 31  ×521\n  [1, 1, 1, 1, 1]  0b11111 = 31\n[0, 0, 0, 0, 0]  0b00000 = 0  ×479\n  [0, 0, 0, 0, 0]  0b00000 = 0',
      note: 'Each row shows the bits as an array, in binary (highest bit first) and as an integer (first bit least significant).',
    },
  },
  {
    name: 'teleport',
    summary: 'Quantum teleportation of |1⟩: measured bits steer the corrections applied to the third qubit.',
    command: 'qxide run examples/qx/teleport.qx --shots 1000 --seed 0',
    result: {
      label: 'Output',
      text: '[1, 0, 1]  0b101 = 5  ×273\n  bob=1\n[0, 0, 1]  0b100 = 4  ×251\n  bob=1\n[1, 1, 1]  0b111 = 7  ×248\n  bob=1\n[0, 1, 1]  0b110 = 6  ×228\n  bob=1',
      note: 'The measured bits vary, but bob is 1 in every outcome.',
    },
  },
  {
    name: 'functions',
    summary: 'bellPair borrows two qubits and entangles them, while measureAll takes ownership of the register with consume and measures it.',
    command: 'qxide run examples/qx/functions.qx --shots 1000 --seed 0',
    result: {
      label: 'Output',
      text: "[1, 1]  0b11 = 3  ×521\n  [1, 1]\n[0, 0]  0b00 = 0  ×479\n  [0, 0]",
      note: 'This produces the same Bell correlation as bell.qx, even though main never measures the register itself.',
    },
  },
  {
    name: 'rotation',
    summary: 'Uses var, PI, and an (int) cast: four ry rotations of PI / 8 add up to 90 degrees.',
    command: 'qxide run examples/qx/rotation.qx --shots 1000 --seed 0',
    result: {
      label: 'Output',
      text: "[1]  0b1 = 1  ×518\n  ry total 90 degrees (90.0), m=1\n[0]  0b0 = 0  ×482\n  ry total 90 degrees (90.0), m=0",
      note: 'A 90-degree ry rotation produces an even split between 0 and 1, and (int) truncates the printed angle to 90.',
    },
  },
  {
    name: 'teleport_fn',
    summary: 'Quantum teleportation built from functions: prepareOne returns a qubit, and the corrections correctX and correctZ are called inside if branches on measured bits.',
    command: 'qxide run examples/qx/teleport_fn.qx --shots 1000 --seed 0',
    result: {
      label: 'Output',
      text: "[1, 0, 1]  0b101 = 5  ×273\n  bob=1\n[0, 0, 1]  0b100 = 4  ×251\n  bob=1\n[1, 1, 1]  0b111 = 7  ×248\n  bob=1\n[0, 1, 1]  0b110 = 6  ×228\n  bob=1",
      note: 'bob is always 1, and qxide qasm outputs the exact same OpenQASM 3 as teleport.qx.',
    },
  },
  {
    name: 'conditional_fn',
    summary: 'Calls gate-only functions inside if and else branches on measured bits, including a loop of rz rotations, a nested if on a Bit parameter, and a classical helper.',
    command: 'qxide run examples/qx/conditional_fn.qx --shots 1000 --seed 0',
    result: {
      label: 'Output',
      text: "[1, 0, 0, 0]  0b0001 = 1  ×268\n  m=1 f=0 r=[0, 0]\n[1, 1, 1, 0]  0b0111 = 7  ×250\n  m=1 f=1 r=[1, 0]\n[0, 1, 0, 0]  0b0010 = 2  ×246\n  m=0 f=1 r=[0, 0]\n[0, 0, 0, 0]  0b0000 = 0  ×236\n  m=0 f=0 r=[0, 0]",
      note: 'r[0] is 1 only when both coin bits are 1, because the two x gates in the else branch cancel each other out.',
    },
  },
];

// Rust examples in examples/ at v0.7.0 (output text in English since v0.6.0). Results were captured by running the commands unless noted.
const EXAMPLES: Example[] = [
  {
    name: 'bell',
    summary: 'A Bell state: entangle two qubits, print the OpenQASM 3, then simulate 1000 shots and count outcomes.',
    command: 'cargo run --example bell',
    result: {
      label: 'Last lines of output',
      text: '00: 482\n01: 0\n10: 0\n11: 518',
      note: '01 and 10 never occur because the qubits are entangled.',
    },
  },
  {
    name: 'ghz_large',
    summary:
      'A GHZ state on a register of n qubits (default 5000) using the stabilizer simulator, which handles Clifford-only circuits with thousands of qubits.',
    command: 'cargo run --release --example ghz_large 5000',
    result: {
      label: 'Output (timing varies by machine)',
      text: 'seed 0 | 5000 qubits, measured 1: 5000 (333.2127ms)\nseed 1 | 5000 qubits, measured 1: 5000 (340.9436ms)\nseed 2 | 5000 qubits, measured 1: 0 (332.8137ms)',
      note: 'Each seed gives all ones or all zeros, never a mix. Timings are from one Windows laptop.',
    },
  },
  {
    name: 'qasm_dump',
    summary:
      'Writes OpenQASM 3 for ten sample circuits to a directory (default qasm-out): bell, gates, teleport, nested_branches, reset_and_reuse, ghz_register, no_measurement, ancilla_parity, extreme_angles and empty. CI parses them with the OpenQASM 3 reference parser and Qiskit.',
    command: 'cargo run --example qasm_dump -- qasm-out',
    result: {
      label: 'Output',
      text: 'qasm-out/bell.qasm\nqasm-out/gates.qasm\nqasm-out/teleport.qasm\n…',
      note: 'It prints one path per file; the files hold the QASM.',
    },
  },
  {
    name: 'gpu_bench',
    summary:
      'Benchmarks the GPU state-vector prototype against the single-threaded CPU backend and prints CSV (mode, qubits, gates, cpu_ms, gpu_ms, speedup). Needs the gpu feature and a GPU.',
    command: 'cargo run --release --features gpu --example gpu_bench 26',
    note: 'Not run for this page. The README reports roughly 11x faster than a single-threaded CPU on 24 to 26 qubit gate-only circuits; see docs/gpu.md for the method.',
  },
];

export function Examples() {
  usePageMeta(
    'Qxide examples: Bell, GHZ, teleportation, OpenQASM and GPU benchmark',
    'Runnable .qx programs and Rust examples from the Qxide repository, with the command to run each one, a real result and a link to the source.',
  );

  return (
    <article className="prose">
      <header className="page-head">
        <h1>Examples</h1>
        <p className="lede">
          The repository&apos;s{' '}
          <a href={`${GITHUB_TREE}/examples`} target="_blank" rel="noreferrer">
            examples/
          </a>{' '}
          folder has seven <code>.qx</code> programs and four Rust examples. Clone the repository and run any command
          below from its root.
        </p>
      </header>

      <CodeBlock
        label="Get the code"
        lang="sh"
        code={'git clone https://github.com/qxide-lang/qxide\ncd qxide'}
      />

      <h2 className="section-title">.qx programs</h2>
      <p>
        These need the <code>qxide</code> command, which you can install from the{' '}
        <Link to="/learn#install">Learn page</Link>.
      </p>
      <div className="card-list">
        {QX_EXAMPLES.map((ex) => (
          <article className="card" key={ex.name} aria-labelledby={`qx-${ex.name}`}>
            <h2 id={`qx-${ex.name}`}>
              <code>{ex.name}.qx</code>
            </h2>
            <p>{ex.summary}</p>
            <CodeBlock label={`Run ${ex.name}.qx`} lang="sh" code={ex.command} />
            {ex.result && (
              <>
                <CodeBlock label={ex.result.label} copyable={false} code={ex.result.text} />
                <p className="card-note">{ex.result.note}</p>
              </>
            )}
            <p>
              <a href={`${GITHUB_BLOB}/examples/qx/${ex.name}.qx`} target="_blank" rel="noreferrer">
                View examples/qx/{ex.name}.qx on GitHub<span className="sr-only"> (opens in a new tab)</span>
              </a>
            </p>
          </article>
        ))}
      </div>

      <h2 className="section-title">Rust library examples</h2>
      <p>
        These use the <code>qxide</code> crate directly and are run with <code>cargo run --example</code>.
      </p>
      <div className="card-list">
        {EXAMPLES.map((ex) => (
          <article className="card" key={ex.name} aria-labelledby={`ex-${ex.name}`}>
            <h2 id={`ex-${ex.name}`}>
              <code>{ex.name}</code>
            </h2>
            <p>{ex.summary}</p>
            <CodeBlock label={`Run ${ex.name}`} lang="sh" code={ex.command} />
            {ex.result && (
              <>
                <CodeBlock label={ex.result.label} copyable={false} code={ex.result.text} />
                <p className="card-note">{ex.result.note}</p>
              </>
            )}
            {ex.note && <p className="card-note">{ex.note}</p>}
            <p>
              <a href={`${GITHUB_BLOB}/examples/${ex.name}.rs`} target="_blank" rel="noreferrer">
                View examples/{ex.name}.rs on GitHub<span className="sr-only"> (opens in a new tab)</span>
              </a>
            </p>
          </article>
        ))}
      </div>
    </article>
  );
}
