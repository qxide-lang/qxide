import {Link} from 'react-router';
import {CodeBlock} from '../components/CodeBlock';
import {GITHUB_BLOB, GITHUB_TREE, usePageMeta} from '../lib/meta';

const BELL = `// examples/qx/bell.qx
void main() {
    Qubit a = qubit();
    Qubit b = qubit();
    h(a);
    cx(a, b);
    Bit ma = measure(a);
    Bit mb = measure(b);
    println("a={} b={}", ma, mb);
}`;

const BELL_RUN = `$ qxide run examples/qx/bell.qx
a=1 b=1`;

const BELL_SHOTS = `$ qxide run examples/qx/bell.qx --shots 1000
[1, 1]  0b11 = 3  ×521
  a=1 b=1
[0, 0]  0b00 = 0  ×479
  a=0 b=0`;

const GHZ = `// examples/qx/ghz.qx
void main() {
    int n = 5;
    QReg r = qreg(n);
    h(r[0]);
    for i in 1..n {
        cx(r[0], r[i]);
    }
    BitReg m = measure(r);
    println("{}  {} = {}", m, m.to_binary(), m.as_int());
}`;

const GHZ_OUT = `$ qxide run examples/qx/ghz.qx --shots 1000
[1, 1, 1, 1, 1]  0b11111 = 31  ×521
  [1, 1, 1, 1, 1]  0b11111 = 31
[0, 0, 0, 0, 0]  0b00000 = 0  ×479
  [0, 0, 0, 0, 0]  0b00000 = 0`;

const TELEPORT = `// examples/qx/teleport.qx
void main() {
    Qubit msg = qubit();
    Qubit alice = qubit();
    Qubit bob = qubit();
    x(msg);
    h(alice);
    cx(alice, bob);
    cx(msg, alice);
    h(msg);
    Bit m0 = measure(msg);
    Bit m1 = measure(alice);
    if m1 {
        x(bob);
    }
    if m0 {
        z(bob);
    }
    Bit out = measure(bob);
    println("bob={}", out);
}`;

const TELEPORT_OUT = `$ qxide run examples/qx/teleport.qx --shots 1000
[1, 0, 1]  0b101 = 5  ×273
  bob=1
[0, 0, 1]  0b100 = 4  ×251
  bob=1
[1, 1, 1]  0b111 = 7  ×248
  bob=1
[0, 1, 1]  0b110 = 6  ×228
  bob=1`;

const BAD = `void main() {
    Qubit q = qubit();
    Bit m = measure(q);
    h(q);
}`;

const BAD_OUT = `$ qxide check use_after_measure.qx
error[Q0301]: use of consumed qubit \`q\`
 --> use_after_measure.qx:4:7
  |
4 |     h(q);
  |       ^
note: qubit consumed here
 --> use_after_measure.qx:3:13
  |
3 |     Bit m = measure(q);
  |             ^^^^^^^^^^`;

const QASM = `OPENQASM 3.0;
include "stdgates.inc";
qubit[2] q;
bit[2] c;
h q[0];
cx q[0], q[1];
c[0] = measure q[0];
c[1] = measure q[1];`;

const RUST_BELL = `use qxide::Circuit;

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

const STEPS = [
  ['what-is-a-qubit', 'What is a qubit?'],
  ['install', 'Install'],
  ['first-program', 'Your first program'],
  ['read-results', 'Reading the results'],
  ['ghz', 'Scaling up: GHZ'],
  ['teleport', 'Using measurement results'],
  ['safety', 'Qubit safety, checked early'],
  ['qasm', 'Export OpenQASM 3'],
  ['rust', 'The Rust library'],
  ['next', 'Where to go next'],
];

export function Learn() {
  usePageMeta(
    'Learn Qxide: from qubits to your first .qx program',
    'A hands-on tutorial: what a qubit is, installing Qxide, writing a Bell state in .qx, reading measurements, GHZ states, teleportation, compile-time qubit checks and OpenQASM 3 export.',
  );

  return (
    <article className="prose">
      <header className="page-head">
        <h1>Learn Qxide</h1>
        <p className="lede">
          A short tutorial for people new to quantum computing. Every snippet is a <code>.qx</code> program (or Rust code, where marked) that exists today, and every output shown was captured by running it with Qxide 0.6.0.
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
          Quantum physics also has rules, and Qxide checks them before your program runs, much as Rust&apos;s
          ownership rules do for memory. A qubit can&apos;t be copied. Measuring destroys the quantum state, so a
          measured qubit can&apos;t be used again. A two-qubit gate needs two <em>different</em> qubits, so{' '}
          <code>cx(a, a)</code> is an error. And every qubit must be measured or explicitly discarded before it goes
          out of scope.
        </p>
      </section>

      <section aria-labelledby="install">
        <h2 id="install">Install</h2>
        <p>
          The <code>qxide</code> command installs from a prebuilt binary. On macOS or Linux:
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
          Check it worked with <code>qxide --version</code>. <code>cargo install qxide</code> and{' '}
          <code>pip install qxide</code> install the same command-line tool. The example programs below are in the
          repository&apos;s{' '}
          <a href={`${GITHUB_TREE}/examples/qx`} target="_blank" rel="noreferrer">
            examples/qx
          </a>{' '}
          folder; save a snippet to a <code>.qx</code> file to run it.
        </p>
      </section>

      <section aria-labelledby="first-program">
        <h2 id="first-program">Your first program: a Bell state</h2>
        <p>
          This is{' '}
          <a href={`${GITHUB_BLOB}/examples/qx/bell.qx`} target="_blank" rel="noreferrer">
            examples/qx/bell.qx
          </a>
          . Declarations look like Java or C: <code>Qubit a = qubit();</code>, <code>void main()</code>.
        </p>
        <CodeBlock label="bell.qx" lang="qx" code={BELL} />
        <p>
          <code>h</code> puts <code>a</code> into superposition. <code>cx</code> (controlled-NOT) flips{' '}
          <code>b</code> when <code>a</code> is 1, which entangles them. <code>measure</code> consumes each qubit
          and hands back a classical <code>Bit</code>. Run it:
        </p>
        <CodeBlock label="Run bell.qx" copyable={false} code={BELL_RUN} />
      </section>

      <section aria-labelledby="read-results">
        <h2 id="read-results">Reading the results</h2>
        <p>
          One shot gives one random outcome. Ask for many shots with <code>--shots</code> and Qxide groups them by
          outcome:
        </p>
        <CodeBlock label="1000 shots" copyable={false} code={BELL_SHOTS} />
        <p>
          Each row is one measurement record followed by how often it occurred. The bits are shown three ways: as an
          array with the first measured bit first (<code>[1, 1]</code>), in binary with the highest bit first (
          <code>0b11</code>), and as an integer with the first bit as the least significant bit (<code>= 3</code>).
          The program&apos;s own output for that outcome is indented below. About half the shots read{' '}
          <code>[0, 0]</code> and half <code>[1, 1]</code>; <code>[0, 1]</code> and <code>[1, 0]</code> never appear.
          That is the entanglement: each bit alone is a coin flip, but they always agree. Use <code>--seed</code> for
          a repeatable run.
        </p>
      </section>

      <section aria-labelledby="ghz">
        <h2 id="ghz">Scaling up: GHZ</h2>
        <p>
          Entangle more than two qubits by applying <code>cx</code> from one head qubit to each of the rest. A
          register (<code>QReg</code>) holds many qubits, and <code>measure</code> on it returns a{' '}
          <code>BitReg</code>. This is{' '}
          <a href={`${GITHUB_BLOB}/examples/qx/ghz.qx`} target="_blank" rel="noreferrer">
            examples/qx/ghz.qx
          </a>
          .
        </p>
        <CodeBlock label="ghz.qx" lang="qx" code={GHZ} />
        <CodeBlock label="Output" copyable={false} code={GHZ_OUT} />
        <p>
          Every run is all zeros or all ones, never a mix. <code>m.to_binary()</code> prints the register with the
          highest bit first and <code>m.as_int()</code> turns it into a number, so all ones reads <code>0b11111</code>{' '}
          and <code>31</code>.
        </p>
      </section>

      <section aria-labelledby="teleport">
        <h2 id="teleport">Using measurement results</h2>
        <p>
          A measured <code>Bit</code> is ordinary classical data, so you can branch on it. In quantum teleportation,
          the two measured bits decide which corrections to apply to the third qubit. This is{' '}
          <a href={`${GITHUB_BLOB}/examples/qx/teleport.qx`} target="_blank" rel="noreferrer">
            examples/qx/teleport.qx
          </a>
          , which teleports the state |1⟩:
        </p>
        <CodeBlock label="teleport.qx" lang="qx" code={TELEPORT} />
        <CodeBlock label="Output" copyable={false} code={TELEPORT_OUT} />
        <p>
          The three measured bits vary from shot to shot, but <code>bob=1</code> every time: the state arrived. In
          OpenQASM 3, an <code>if</code> on a measured bit becomes a classical feed-forward branch.
        </p>
      </section>

      <section aria-labelledby="safety">
        <h2 id="safety">Qubit safety, checked early</h2>
        <p>
          Here the program uses <code>q</code> after measuring it:
        </p>
        <CodeBlock label="use_after_measure.qx" lang="qx" code={BAD} />
        <p>
          <code>qxide check</code> reports it with a line and column, before anything runs:
        </p>
        <CodeBlock label="Output" copyable={false} code={BAD_OUT} />
        <p>
          The checker also rejects a qubit that is never measured or discarded, and the same qubit passed twice to one
          gate. Errors have codes (<code>Q01xx</code> to <code>Q04xx</code>) and <code>note</code> or{' '}
          <code>help</code> lines.
        </p>
      </section>

      <section aria-labelledby="qasm">
        <h2 id="qasm">Export OpenQASM 3</h2>
        <p>
          <code>qxide qasm bell.qx</code> prints the circuit as OpenQASM 3, a text format that quantum software and
          hardware services read. For the Bell program it is:
        </p>
        <CodeBlock label="OpenQASM 3 for bell.qx" copyable={false} code={QASM} />
        <p>
          Qxide does not yet submit jobs to real quantum hardware, so for now you take this text to another tool.
          CI checks that the OpenQASM 3 generated from the example programs parses with Qiskit.
        </p>
      </section>

      <section aria-labelledby="rust">
        <h2 id="rust">The Rust library</h2>
        <p>
          The same engine is the <code>qxide</code> Rust crate, where the Rust compiler enforces the ownership rules
          directly: <code>Qubit</code> has no <code>Clone</code> or <code>Copy</code>, <code>measure</code> takes the
          qubit by value, and <code>cx(&amp;mut a, &amp;mut b)</code> won&apos;t compile with the same qubit twice.
          Add it with <code>cargo add qxide</code>. This is the Bell circuit from{' '}
          <a href={`${GITHUB_BLOB}/examples/bell.rs`} target="_blank" rel="noreferrer">
            examples/bell.rs
          </a>
          , trimmed:
        </p>
        <CodeBlock label="src/main.rs (Rust)" lang="rust" code={RUST_BELL} />
        <p>
          The library has features the <code>.qx</code> language does not have yet. One is <code>with_ancilla</code>:
          it computes into a temporary helper qubit, lets you use the result, then appends the reverse of the
          computation so the helper returns to 0. In the second closure the inputs are only available as controls.
        </p>
        <CodeBlock label="src/main.rs (Rust)" lang="rust" code={ANCILLA} />
        <p>
          Its measured result is <code>a=1 b=0 target=1</code>: the target flipped because the parity of 1 and 0 is
          1, and the inputs were left intact. Details are in the{' '}
          <a href={`${GITHUB_BLOB}/docs/design/uncomputation.md`} target="_blank" rel="noreferrer">
            uncomputation design doc
          </a>
          , and the full API is on{' '}
          <a href="https://docs.rs/qxide" target="_blank" rel="noreferrer">
            docs.rs
          </a>
          .
        </p>
      </section>

      <section aria-labelledby="next">
        <h2 id="next">Where to go next</h2>
        <ul className="plain-list">
          <li>
            <Link to="/examples">Examples</Link>: every runnable example in the repository, with run commands.
          </li>
          <li>
            <Link to="/docs">Docs</Link>: the command line, the language design document and the API reference.
          </li>
          <li>
            <Link to="/blog/qxide-0-6-0-functions-with-qubits">Qxide 0.6.0</Link>: functions that take and return qubits, conditional calls, <code>var</code>, casts, and <code>PI</code>.
          </li>
        </ul>
      </section>
    </article>
  );
}
