import {Link} from 'react-router';
import {CodeBlock} from '../components/CodeBlock';
import {GITHUB_BLOB, usePageMeta} from '../lib/meta';

const BELL = `use qxide::Circuit;

fn main() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let mut b = c.qubit();

    c.h(&mut a); // a becomes a mix of 0 and 1
    c.cx(&mut a, &mut b); // a and b are now entangled

    let ma = c.measure(a); // a and b are consumed here;
    let mb = c.measure(b); // only classical bits remain

    println!("{}", c.to_qasm());
    let shot = c.simulate(42);
    println!("a={} b={}", shot.get(ma) as u8, shot.get(mb) as u8);
}`;

const COUNTS = `let mut counts = [0u32; 4];
for seed in 0..1000 {
    let shot = c.simulate(seed);
    let k = (shot.get(ma) as usize) << 1 | shot.get(mb) as usize;
    counts[k] += 1;
}
for (k, n) in counts.iter().enumerate() {
    println!("{}{}: {n}", k >> 1, k & 1);
}`;

const GHZ = `use qxide::{Backend, Circuit};

fn main() {
    let n = 5;
    let mut c = Circuit::new();
    let mut r = c.qreg(n);
    let (head, rest) = r.split_first_mut().unwrap();
    c.h(head);
    for t in rest {
        c.cx(head, t);
    }
    let bits = c.measure_reg(r);

    for seed in 0..3 {
        let shot = c.simulate_with(Backend::Stabilizer, seed).unwrap();
        let ones = bits.iter().filter(|&&b| shot.get(b)).count();
        println!("seed {seed} | {n} qubits, ones: {ones}");
    }
}`;

const ANCILLA = `use qxide::Circuit;

fn main() {
    let mut c = Circuit::new();
    let mut a = c.qubit();
    let b = c.qubit();
    let mut target = c.qubit();
    c.x(&mut a); // a = 1, b = 0

    // Compute the parity of a and b into a helper qubit, use it to flip
    // \`target\`, then let qxide undo the computation.
    c.with_ancilla(
        |u, anc| {
            u.cx(&a, anc);
            u.cx(&b, anc);
        },
        |c, anc| c.cx_ctrl(anc, &mut target),
    );

    let (ma, mb, mt) = (c.measure(a), c.measure(b), c.measure(target));
    print!("{}", c.to_qasm());
    let s = c.simulate(0);
    println!("a={} b={} target={}", s.get(ma) as u8, s.get(mb) as u8, s.get(mt) as u8);
}`;

const ANCILLA_OUT = `OPENQASM 3.0;
include "stdgates.inc";
qubit[4] q;
bit[3] c;
x q[0];
cx q[0], q[3];
cx q[1], q[3];
cx q[3], q[2];
cx q[1], q[3];
cx q[0], q[3];
c[0] = measure q[0];
c[1] = measure q[1];
c[2] = measure q[2];
a=1 b=0 target=1`;

const QASM = `OPENQASM 3.0;
include "stdgates.inc";
qubit[2] q;
bit[2] c;
h q[0];
cx q[0], q[1];
c[0] = measure q[0];
c[1] = measure q[1];`;

const STEPS = [
  ['what-is-a-qubit', 'What is a qubit?'],
  ['install', 'Install'],
  ['first-circuit', 'Your first circuit'],
  ['read-results', 'Reading the results'],
  ['ghz', 'Scaling up: GHZ'],
  ['ancilla', 'Undoing work with with_ancilla'],
  ['qasm', 'Export OpenQASM 3'],
  ['next', 'Where to go next'],
];

export function Learn() {
  usePageMeta(
    'Learn Qxide: from qubits to your first circuit',
    'A hands-on tutorial: what a qubit is, installing Qxide, building a Bell state, reading measurements, GHZ states, with_ancilla uncomputation and OpenQASM 3 export.',
  );

  return (
    <article className="prose">
      <header className="page-head">
        <h1>Learn Qxide</h1>
        <p className="lede">
          A short tutorial for people new to quantum computing. Every snippet uses the Rust API and the{' '}
          <code>qxide</code> command that exist today, and every output shown was captured by running the code.
        </p>
      </header>

      <nav className="toc" aria-label="On this page">
        <ol>
          {STEPS.map(([id, title]) => (
            <li key={id}>
              <a href={`#${id}`}>{title}</a>
            </li>
          ))}
        </ol>
      </nav>

      <section aria-labelledby="what-is-a-qubit">
        <h2 id="what-is-a-qubit">What is a qubit?</h2>
        <p>
          A bit is 0 or 1. A <strong>qubit</strong> is the quantum version: before you look at it, it can be a
          blend of 0 and 1 (a <em>superposition</em>). When you <em>measure</em> it, you get a plain 0 or 1, and the
          blend is gone.
        </p>
        <p>
          Two qubits can also be <strong>entangled</strong>: their measurement results are linked, even though each
          one alone looks random. You will see that in a moment.
        </p>
        <p>
          Quantum physics also has rules, and Qxide turns them into Rust&apos;s ownership rules. A qubit can&apos;t be
          copied, so <code>Qubit</code> has no <code>Clone</code> or <code>Copy</code>. Measuring destroys the
          quantum state, so <code>measure</code> takes the qubit by value and you can&apos;t use it again. A two-qubit
          gate needs two <em>different</em> qubits, so <code>cx(&amp;mut a, &amp;mut b)</code> won&apos;t compile
          if you pass the same one twice.
        </p>
      </section>

      <section aria-labelledby="install">
        <h2 id="install">Install</h2>
        <p>
          The command-line tool installs from a prebuilt binary. On macOS or Linux:
        </p>
        <CodeBlock
          label="Install script (macOS / Linux)"
          lang="sh"
          code="curl --proto '=https' --tlsv1.2 -LsSf https://qxide.dev/install.sh | sh"
        />
        <p>On Windows (PowerShell):</p>
        <CodeBlock
          label="Install script (Windows)"
          lang="sh"
          code='powershell -ExecutionPolicy Bypass -c "irm https://qxide.dev/install.ps1 | iex"'
        />
        <p>
          Check it worked with <code>qxide --version</code>. To use the library in your own Rust project, add the
          crate:
        </p>
        <CodeBlock label="Add the crate" lang="sh" code="cargo add qxide" />
        <p>
          <code>cargo install qxide</code> and <code>pip install qxide</code> also install the same command-line
          tool.
        </p>
      </section>

      <section aria-labelledby="first-circuit">
        <h2 id="first-circuit">Your first circuit: a Bell state</h2>
        <p>
          Create a project with <code>cargo new hello-qxide</code>, run <code>cargo add qxide</code> inside it, and
          put this in <code>src/main.rs</code>. It is the same circuit as{' '}
          <a href={`${GITHUB_BLOB}/examples/bell.rs`} target="_blank" rel="noreferrer">
            examples/bell.rs
          </a>
          , trimmed.
        </p>
        <CodeBlock label="src/main.rs" lang="rust" code={BELL} />
        <p>
          <code>h</code> puts <code>a</code> into superposition. <code>cx</code> (controlled-NOT) flips{' '}
          <code>b</code> when <code>a</code> is 1, which entangles them. <code>measure</code> consumes each qubit
          and hands back a classical <code>Bit</code>. Nothing runs until you call <code>simulate</code>.
        </p>
      </section>

      <section aria-labelledby="read-results">
        <h2 id="read-results">Reading the results</h2>
        <p>
          One shot gives one random outcome. Run the circuit with a thousand different seeds and count what comes
          out (this loop is also in <code>examples/bell.rs</code>):
        </p>
        <CodeBlock label="Count 1000 shots" lang="rust" code={COUNTS} />
        <CodeBlock
          label="Output"
          copyable={false}
          code={`00: 503
01: 0
10: 0
11: 497`}
        />
        <p>
          About half the shots read <code>00</code> and half read <code>11</code>. <code>01</code> and{' '}
          <code>10</code> never appear: that is the entanglement. Each bit on its own is a coin flip, but they always
          agree. The same seed always gives the same result. You can get this exact output without writing code by
          running <code>qxide bell</code>.
        </p>
      </section>

      <section aria-labelledby="ghz">
        <h2 id="ghz">Scaling up: GHZ</h2>
        <p>
          Entangle more than two qubits by applying <code>cx</code> from one head qubit to each of the rest. A
          register (<code>qreg</code>) holds many qubits; this is the circuit in{' '}
          <a href={`${GITHUB_BLOB}/examples/ghz_large.rs`} target="_blank" rel="noreferrer">
            examples/ghz_large.rs
          </a>{' '}
          with <code>n</code> fixed at 5 and the timing removed.
        </p>
        <CodeBlock label="src/main.rs" lang="rust" code={GHZ} />
        <CodeBlock
          label="Output"
          copyable={false}
          code={`seed 0 | 5 qubits, ones: 5
seed 1 | 5 qubits, ones: 5
seed 2 | 5 qubits, ones: 0`}
        />
        <p>
          Every run is all ones or all zeros, never a mix. This circuit only uses Clifford gates, so the stabilizer
          simulator can run it with thousands of qubits (the full example defaults to 5000). The state-vector
          simulator stores 2<sup>n</sup> amplitudes, so memory doubles with every qubit; see the{' '}
          <a href={`${GITHUB_BLOB}/README.md`} target="_blank" rel="noreferrer">
            README table
          </a>
          .
        </p>
      </section>

      <section aria-labelledby="ancilla">
        <h2 id="ancilla">Undoing work with with_ancilla</h2>
        <p>
          Many quantum algorithms compute something into a temporary helper qubit (an <em>ancilla</em>), use it,
          then must undo the computation so the helper returns to 0 and can be reused. <code>with_ancilla</code> does
          that for you: the first closure computes, the second uses the result, and Qxide appends the reverse of the
          first closure. In the second closure the inputs are only available as controls.
        </p>
        <CodeBlock label="src/main.rs" lang="rust" code={ANCILLA} />
        <CodeBlock label="Output" copyable={false} code={ANCILLA_OUT} />
        <p>
          Look at the gates on <code>q[3]</code>, the helper. The two <code>cx</code> gates that computed the parity
          are repeated in reverse order after the target is flipped, so the helper is back at 0. The measured result
          is <code>a=1 b=0 target=1</code>: the target flipped because the parity of 1 and 0 is 1, and the inputs
          were left intact. Details are in the{' '}
          <a href={`${GITHUB_BLOB}/docs/design/uncomputation.md`} target="_blank" rel="noreferrer">
            uncomputation design doc
          </a>
          .
        </p>
      </section>

      <section aria-labelledby="qasm">
        <h2 id="qasm">Export OpenQASM 3</h2>
        <p>
          <code>c.to_qasm()</code> returns the circuit as OpenQASM 3, a text format that quantum software and
          hardware services read. You already printed it in the first circuit. For the Bell circuit it is:
        </p>
        <CodeBlock label="OpenQASM 3 for the Bell circuit" copyable={false} code={QASM} />
        <p>
          Qxide does not yet submit jobs to real quantum hardware, so for now you take this text to another tool.
          To write the QASM for several sample circuits to files, run{' '}
          <code>cargo run --example qasm_dump -- qasm-out</code> in the repository. CI checks that those files parse
          with Qiskit.
        </p>
      </section>

      <section aria-labelledby="next">
        <h2 id="next">Where to go next</h2>
        <ul className="plain-list">
          <li>
            <Link to="/examples">Examples</Link>: every runnable example in the repository, with run commands.
          </li>
          <li>
            <Link to="/docs">Docs</Link>: API reference on docs.rs, design notes and the CLI summary.
          </li>
          <li>
            <Link to="/blog/why-qxide">Why Qxide</Link>: where the project is headed, including the planned{' '}
            <code>.qx</code> language.
          </li>
        </ul>
      </section>
    </article>
  );
}
