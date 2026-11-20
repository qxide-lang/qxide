import {CodeBlock} from '../components/CodeBlock';
import {GITHUB_BLOB, GITHUB_TREE, usePageMeta} from '../lib/meta';

type Example = {
  name: string;
  summary: string;
  command: string;
  result?: {label: string; text: string; note: string};
  note?: string;
};

// Every example in examples/ at v0.4.1. Results were captured by running the commands unless noted.
const EXAMPLES: Example[] = [
  {
    name: 'bell',
    summary: 'A Bell state: entangle two qubits, print the OpenQASM 3, then simulate 1000 shots and count outcomes.',
    command: 'cargo run --example bell',
    result: {
      label: 'Last lines of output',
      text: '00: 503\n01: 0\n10: 0\n11: 497',
      note: '01 and 10 never occur because the qubits are entangled.',
    },
  },
  {
    name: 'ghz_large',
    summary:
      'A GHZ state on a register of n qubits (default 5000) using the stabilizer simulator, which handles Clifford-only circuits with thousands of qubits.',
    command: 'cargo run --release --example ghz_large 5000',
    result: {
      label: 'Output (the example prints Korean; timing varies by machine)',
      text: '시드 0 | 5000큐비트, 측정값 1: 5000개 (483.3946ms)\n시드 1 | 5000큐비트, 측정값 1: 5000개 (424.9412ms)\n시드 2 | 5000큐비트, 측정값 1: 0개 (441.7877ms)',
      note: 'Each seed gives all ones or all zeros, never a mix. Timings are from one Windows laptop.',
    },
  },
  {
    name: 'qasm_dump',
    summary:
      'Writes OpenQASM 3 for eight sample circuits to a directory (default qasm-out): bell, gates, teleport, nested_branches, reset_and_reuse, ghz_register, no_measurement and ancilla_parity. CI parses them with Qiskit.',
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
    'Qxide examples: Bell, GHZ, OpenQASM and GPU benchmark',
    'Runnable examples from the Qxide repository, with the command to run each one, a real result and a link to the source.',
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
          folder has four runnable programs. Clone the repository and run any command below from its root.
        </p>
      </header>

      <CodeBlock
        label="Get the code"
        lang="sh"
        code={'git clone https://github.com/qxide-lang/qxide\ncd qxide'}
      />

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
