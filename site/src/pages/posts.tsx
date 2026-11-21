import type {ReactNode} from 'react';
import {Link} from 'react-router';
import {CodeBlock} from '../components/CodeBlock';
import {GITHUB, GITHUB_BLOB} from '../lib/meta';

export type Post = {
  slug: string;
  title: string;
  /** ISO date (YYYY-MM-DD). */
  date: string;
  summary: string;
  body: ReactNode;
};

const RELEASE_070 = `${GITHUB}/releases/tag/v0.7.0`;

const setup070 = `$ qxide new demo
Created project \`demo\` in \`demo\` with \`qxide.toml\`, \`src/main.qx\`, \`README.md\`, and \`.gitignore\`
Next:
  qxide run demo/src/main.qx
  qxide fmt demo/src/main.qx
$ cd demo
$ qxide add bell-utils
       Added bell-utils v0.1.0
      Locked 1 packages
  Downloaded bell-utils v0.1.0`;

const main070 = `import bell_utils.bell_pair;
import bell_utils.plus;

void main() {
    Qubit a = qubit();
    Qubit b = qubit();
    bell_pair(a, b);
    Bit ma = measure(a);
    Bit mb = measure(b);
    Qubit p = plus();
    Bit mp = measure(p);
    println("a={} b={} plus={}", ma, mb, mp);
}`;

const run070 = `$ qxide run --shots 1000
[1, 1, 0]  0b011 = 3  ×273
  a=1 b=1 plus=0
[0, 0, 0]  0b000 = 0  ×251
  a=0 b=0 plus=0
[1, 1, 1]  0b111 = 7  ×248
  a=1 b=1 plus=1
[0, 0, 1]  0b100 = 4  ×228
  a=0 b=0 plus=1`;

const release070 = (
  <>
    <p>Qxide 0.7.0 introduces modules and a package manager backed by a GitHub registry, along with the first published package, <code>bell-utils</code>. This release also adds four new commands (<code>qxide new</code>, <code>qxide fmt</code>, <code>qxide repl</code>, and <code>qxide explain</code>) and fixes shot sampling so that every shot uses an independent seed. Qxide is still an early experiment, and both the language and package format may continue to change.</p>

    <h2>Modules and packages</h2>
    <p>A Qxide project is defined by a <code>qxide.toml</code> manifest and a <code>qxide.lock</code> file that pins dependencies with SHA-256 checksums, and commands like <code>qxide run</code>, <code>qxide check</code>, and <code>qxide qasm</code> default to <code>src/main.qx</code> inside one. Packages come from <code>qxide-lang/registry</code>, a GitHub repository holding a JSON-lines index and archives with no separate server or accounts, where <code>bell-utils</code> 0.1.0 is the first published package.</p>
    <CodeBlock label="New project and a package" copyable={false} code={setup070} />
    <p>Every <code>.qx</code> file is a module, and functions marked <code>public</code> can be brought into scope with Java-style imports such as <code>import bell_utils.bell_pair;</code> or <code>import bell_utils.*;</code>. In the <code>src/main.qx</code> program below, <code>a</code> and <code>b</code> always agree in the 1000-shot output because <code>bell_pair(a, b)</code> entangles the two qubits that the caller allocated and still owns before the caller measures them.</p>
    <CodeBlock label="src/main.qx" lang="qx" code={main070} />
    <CodeBlock label="qxide run (output)" copyable={false} code={run070} />
    <p>Publishing a package uses <code>qxide publish</code>, which bundles the project and opens a pull request to <code>qxide-lang/registry</code> with the GitHub CLI <code>gh</code>. CI verifies the JSON-lines index entry and archive before merge, and published versions are immutable and can only be yanked rather than overwritten. Because this is the first release of the package manager, the manifest and registry format may still change in future versions.</p>

    <h2>New commands</h2>
    <p><code>{"qxide new <NAME>"}</code> creates a project with <code>qxide.toml</code>, a Bell-pair program in <code>src/main.qx</code>, <code>README.md</code>, and <code>.gitignore</code>. <code>{"qxide fmt <FILE> [--check]"}</code> formats a file in place while keeping comments, and <code>--check</code> verifies formatting without changing the file. <code>qxide repl</code> opens an interactive session with the same rules as <code>qxide run</code>, and <code>{"qxide explain <CODE>"}</code> prints the explanation, an example, and the fix for an error code.</p>

    <h2>Independent shot sampling</h2>
    <p>Until 0.6.0, shot <code>k</code> under <code>--seed S</code> used seed <code>S + k</code>, so runs with nearby seeds reused almost all of each other's shots. Across seeds <code>0..199</code>, the Bell <code>00</code> count in 1000 shots had a standard deviation of 2.94 instead of about 15.8, and its 506.1 mean came from one frozen sample of shared shots rather than any real bias toward <code>00</code>. Now every shot gets an independent seed through <code>qxide::shot_seed</code> and <code>Circuit::simulate_shots</code>, bringing the standard deviation to 16.7 and the mean to 497.6. The same <code>--seed</code> still reproduces a run, but counts differ from 0.6.0 (the Bell example with seed 0 now prints 521 and 479 instead of 504 and 496), and the playground uses the same code.</p>

    <h2>Breaking changes</h2>
    <p><code>import</code> and <code>public</code> are now keywords, and <code>qxide new</code> creates the new project layout. The compiler now rejects consuming a qubit in the right operand of <code>{"&&"}</code> or <code>||</code> (<code>Q0315</code>) as well as consuming an operand borrowed by the same call (<code>Q0301</code>), and long operator chains count toward the 128-level nesting limit (<code>Q0106</code>). Passing a non-finite angle to <code>rx</code>, <code>ry</code>, or <code>rz</code> now panics in Rust and reports <code>Q0411</code> in <code>.qx</code> files.</p>

    <h2>Install or upgrade</h2>
    <p>
      The install and upgrade commands are on the <Link to="/learn#install">Learn page</Link>
      , and the full list of changes is in the{' '}
      <a href={RELEASE_070} target="_blank" rel="noreferrer">
        v0.7.0 GitHub release
      </a>
       and in{' '}
      <a href={`${GITHUB_BLOB}/CHANGELOG.md`} target="_blank" rel="noreferrer">
        CHANGELOG.md
      </a>
      .
    </p>
  </>
);

const RELEASE_060 = `${GITHUB}/releases/tag/v0.6.0`;

const functions060 = `// Qubit parameters are borrowed by default; \`consume\` hands ownership to the callee.
void bellPair(Qubit a, Qubit b) {
    h(a);
    cx(a, b);
}

void measureAll(consume QReg r) {
    BitReg m = measure(r);
    println("{}", m);
}

void main() {
    QReg r = qreg(2);
    bellPair(r[0], r[1]);
    measureAll(r);
}`;

const functionsRun060 = `$ qxide run examples/qx/functions.qx --shots 1000
[0, 0]  0b00 = 0  ×504
  [0, 0]
[1, 1]  0b11 = 3  ×496
  [1, 1]`;

const teleport060 = `// examples/qx/teleport_fn.qx (excerpt)
void correctX(Qubit q) {
    x(q);
}

void correctZ(Qubit q) {
    z(q);
}

    // in main, after measuring msg (m0) and alice (m1)
    if m1 {
        correctX(bob);
    }
    if m0 {
        correctZ(bob);
    }`;

const teleportQasm060 = `$ qxide qasm examples/qx/teleport_fn.qx
...
c[0] = measure q[0];
c[1] = measure q[1];
if (c[1] == true) {
  x q[2];
}
if (c[0] == true) {
  z q[2];
}
c[2] = measure q[2];`;

const rotation060 = `// \`var\` for classical locals, \`PI\` for angles, \`(int)\` to truncate a double.
void main() {
    var steps = 4;
    var angle = PI / 2 / steps;
    Qubit q = qubit();
    for i in 0..steps {
        ry(q, angle);
    }
    Bit m = measure(q);
    var degrees = angle * steps * 180 / PI;
    println("ry total {} degrees ({}), m={}", (int) degrees, degrees, m);
}`;

const rotationRun060 = `$ qxide run examples/qx/rotation.qx
ry total 90 degrees (90.0), m=0`;

const release060 = (
  <>
    <p>Qxide 0.6.0 adds functions that take and return qubits, gate-only function calls inside <code>if</code> branches on measured bits, local type inference with <code>var</code>, Java-style numeric casts, and the <code>PI</code> constant. Like earlier releases, this is still an early experiment and the language may change. The Rust library API in the <code>qxide</code> crate is unchanged since 0.5.0, though its documentation and messages are now in English.</p>

    <h2>Functions that take and return qubits</h2>
    <p>In <code>examples/qx/functions.qx</code>, <code>Qubit</code> and <code>QReg</code> parameters are borrowed by default so a function can apply gates without taking ownership. Writing <code>consume</code> before the parameter type hands ownership to the function instead, requiring it to measure or discard the qubits on every path and preventing the caller from using them afterwards.</p>
    <CodeBlock label="functions.qx" lang="qx" code={functions060} />
    <CodeBlock label="qxide run (output)" copyable={false} code={functionsRun060} />
    <p>If a function tries to measure, discard, or move a borrowed qubit, the checker rejects the program with error <code>Q0313</code>. Using a qubit or register in the caller after passing it to a <code>consume</code> parameter is also caught at compile time. Functions can also return <code>Qubit</code> and <code>QReg</code> values, transferring ownership to the caller, which must measure or discard the returned qubits.</p>

    <h2>Calling functions inside an <code>if</code> on a measured bit</h2>
    <p>Gate-only helper functions can now be called inside <code>if</code> statements that branch on a measured <code>Bit</code>. In the excerpt from <code>examples/qx/teleport_fn.qx</code> below, the teleportation corrections are written as separate functions and called inside <code>if m1</code> and <code>if m0</code>.</p>
    <CodeBlock label="teleport_fn.qx" lang="qx" code={teleport060} />
    <CodeBlock label="qxide qasm (output)" copyable={false} code={teleportQasm060} />
    <p>Running <code>qxide qasm</code> on this program produces the exact same OpenQASM 3 output as <code>teleport.qx</code>, just as if the gates had been written inline. Only functions that apply gates to borrowed qubits are allowed inside a branch on a measured <code>Bit</code>. Calling a function that measures, allocates, discards, prints, takes <code>consume</code> parameters, or returns qubits is rejected with error <code>Q0314</code>, with a diagnostic pointing at the reason.</p>

    <h2><code>var</code>, casts, and <code>PI</code></h2>
    <p>Qxide 0.6.0 also adds <code>var</code> for inferring <code>int</code>, <code>double</code>, and <code>boolean</code> local variables, a built-in <code>double</code> constant <code>PI</code>, and Java-style numeric casts <code>(int) x</code> and <code>(double) n</code>. In <code>examples/qx/rotation.qx</code> below, four rotations by <code>PI / 8</code> add up to 90 degrees, and <code>(int)</code> truncates the angle toward zero before printing.</p>
    <CodeBlock label="rotation.qx" lang="qx" code={rotation060} />
    <CodeBlock label="qxide run (output)" copyable={false} code={rotationRun060} />

    <h2>Breaking changes</h2>
    <p>In <code>.qx</code> programs, <code>var</code> and <code>consume</code> are now reserved keywords, and <code>PI</code> cannot be declared or assigned. Built-in type names such as <code>int</code> and <code>Qubit</code> can no longer be used as variable, parameter, or function names. The Rust library API in the <code>qxide</code> crate is unchanged since 0.5.0, while its docs.rs documentation, panic messages, and error text are now in English.</p>

    <h2>Install or upgrade</h2>
    <p>
      The install commands are on the <Link to="/learn#install">Learn page</Link>
      , and the full list of changes is in the{' '}
      <a href={RELEASE_060} target="_blank" rel="noreferrer">
        v0.6.0 GitHub release
      </a>
      and in{' '}
      <a href={`${GITHUB_BLOB}/CHANGELOG.md`} target="_blank" rel="noreferrer">
        CHANGELOG.md
      </a>
      .
    </p>
  </>
);

const RELEASE_050 = `${GITHUB}/releases/tag/v0.5.0`;

const bell = `// Bell pair: the two measured bits are always equal.
void main() {
    Qubit a = qubit();
    Qubit b = qubit();
    h(a);
    cx(a, b);
    Bit ma = measure(a);
    Bit mb = measure(b);
    println("a={} b={}", ma, mb);
}`;

const bellRun = `$ qxide run examples/qx/bell.qx --shots 1000
[0, 0]  0b00 = 0  ×504
  a=0 b=0
[1, 1]  0b11 = 3  ×496
  a=1 b=1`;

const install050 = (
  <>
    <p>Qxide 0.5.0 introduces the .qx language for classical and quantum computers, pairing familiar Java/C-style syntax with compile-time qubit safety inspired by ownership in Rust. Alongside the language, the qxide command now supports run, check, and qasm subcommands, while the Rust library API remains unchanged since 0.4.1. Because .qx is still an early experiment, parts of the language may still change in future releases.</p>

    <h2>A first program</h2>
    <p>The listing below shows examples/qx/bell.qx, a complete Bell-pair program written in .qx. It uses Java-style declarations such as Qubit a = qubit(); inside a void main() entry point.</p>
    <CodeBlock label="bell.qx" lang="qx" code={bell} />
    <p>In the multi-shot output table, each row shows a measurement record as an array with the first bit first such as [0, 1, 1], in binary with the highest bit first such as 0b110, and as an integer with the first bit least significant (6), followed by its count. Any output printed by the program for that measurement outcome appears indented beneath its row.</p>
    <CodeBlock label="qxide run (output)" copyable={false} code={bellRun} />

    <h2>Checked before it runs</h2>
    <p>Before anything runs, the checker rejects use after measurement, qubits that are never measured or discarded, and passing the same qubit twice to one gate. Diagnostics follow a rustc-style format with error codes spanning Q01xx to Q04xx, line:column locations, and note and help lines.</p>

    <h2>What is in 0.5.0</h2>
    <p>Here are the main changes included in Qxide 0.5.0:</p>
    <ul className="plain-list">
      <li>
        The <code>.qx</code> language and the <code>qxide run</code>, <code>qxide check</code> and{' '}
        <code>qxide qasm</code> commands.
      </li>
      <li>
        Types <code>int</code>, <code>double</code>, <code>boolean</code>, <code>Qubit</code>, <code>QReg</code>,{' '}
        <code>Bit</code> and <code>BitReg</code>, plus <code>final</code> declarations.
      </li>
      <li>
        Rotation gates <code>rx</code>, <code>ry</code> and <code>rz</code> that take <code>double</code> angles.
      </li>
      <li>
        Register output as <code>[0, 1, 1]</code>, <code>0b110</code> and <code>as_int()</code>, with grouped outcome
        tables for multi-shot runs.
      </li>
      <li>English diagnostics with codes, line and column spans, and note and help lines.</li>
      <li>
        Example programs in <code>examples/qx</code>, and CI that validates the generated OpenQASM 3 with Qiskit.
      </li>
    </ul>

    <h2>Breaking changes</h2>
    <p>The Rust library has no breaking changes, so existing Rust users can upgrade without changing their code. Only early adopters who tried the unreleased .qx MVP syntax on the main branch need to migrate: declarations now use Java-style syntax instead of let, fn, and mut (using them fails with error Q0110 and a migration hint), and functions that return a value now require an explicit return statement.</p>

    <h2>Install or upgrade</h2>
    <p>
      The install commands are on the <Link to="/learn#install">Learn page</Link>, and the{' '}
      <Link to="/learn">tutorial</Link> walks through the programs above. The full list of changes is in the{' '}
      <a href={RELEASE_050} target="_blank" rel="noreferrer">
        v0.5.0 GitHub release
      </a>{' '}
      and in{' '}
      <a href={`${GITHUB_BLOB}/docs/release-notes/v0.5.0.md`} target="_blank" rel="noreferrer">
        docs/release-notes/v0.5.0.md
      </a>
      .
    </p>
  </>
);

const RELEASE = `${GITHUB}/releases/tag/v0.4.1`;

const install041 = (
  <>
    <p>
      Qxide 0.4.1 is the first release you can install as a program, not only as a Rust dependency. There is now a{' '}
      <code>qxide</code> executable, a one-line install script for each platform, and the CLI is also on crates.io and
      PyPI.
    </p>

    <h2>Install with one line</h2>
    <p>On macOS or Linux:</p>
    <CodeBlock
      label="Install script (macOS / Linux)"
      lang="sh"
      code="curl --proto '=https' --tlsv1.2 -LsSf https://qxide.dev/install.sh | sh"
    />
    <p>On Windows, in PowerShell:</p>
    <CodeBlock
      label="Install script (Windows)"
      lang="sh"
      code='powershell -ExecutionPolicy Bypass -c "irm https://qxide.dev/install.ps1 | iex"'
    />
    <p>
      Those short URLs redirect to the installers attached to the{' '}
      <a href={RELEASE} target="_blank" rel="noreferrer">
        v0.4.1 GitHub release
      </a>
      , which are generated by cargo-dist. The scripts put the <code>qxide</code> binary in your Cargo bin directory
      (<code>CARGO_HOME/bin</code>). If you would rather not pipe a script into a shell, the release also lists
      prebuilt archives with checksums for Apple Silicon and Intel macOS, x64 Windows, and x64 and ARM64 Linux.
    </p>

    <h2>Other ways to install</h2>
    <CodeBlock label="From crates.io" lang="sh" code="cargo install qxide" />
    <CodeBlock label="From PyPI" lang="sh" code="pip install qxide" />
    <p>
      The PyPI package is a wheel that carries the same command-line binary, built with maturin. To use Qxide as a
      library in a Rust project, run <code>cargo add qxide</code>.
    </p>

    <h2>What the command does</h2>
    <p>
      The CLI is small on purpose. <code>qxide --version</code> prints the version, and <code>qxide bell</code> prints
      the OpenQASM 3 for an entangled pair of qubits and then runs 1000 simulated shots. It is a quick check that the
      install works and a first look at what a Qxide circuit produces.
    </p>
    <CodeBlock
      label="qxide bell (output)"
      copyable={false}
      code={`OpenQASM 3
OPENQASM 3.0;
include "stdgates.inc";
qubit[2] q;
bit[2] c;
h q[0];
cx q[0], q[1];
c[0] = measure q[0];
c[1] = measure q[1];

Simulation Counts
00: 503
01: 0
10: 0
11: 497`}
    />
    <p>
      Next, the <Link to="/learn">tutorial</Link> builds this circuit by hand in Rust and goes on to larger ones. The
      release process itself is documented in{' '}
      <a href={`${GITHUB_BLOB}/docs/release.md`} target="_blank" rel="noreferrer">
        docs/release.md
      </a>
      .
    </p>
  </>
);

const whyQxide = (
  <>
    <p className="post-update">
      <strong>Update, 2026-11-20:</strong> the <code>.qx</code> language described below as planned shipped in{' '}
      <Link to="/blog/qxide-0-5-0-the-qx-language">Qxide 0.5.0</Link>, with Java/C-style syntax. The rest of this post is
      unchanged and describes the project as of 2026-11-19.
    </p>
    <p>
      Quantum programs are rarely quantum all the way through. A real program mixes ordinary code (loops, numbers,
      decisions based on measurement results) with a quantum part. Most tools today handle that by embedding a circuit
      description inside a general-purpose language. Qxide asks what happens if one language covers both sides and
      keeps the rules of quantum mechanics checkable by the compiler.
    </p>

    <h2>Physics rules that Rust already has</h2>
    <p>Three rules of quantum mechanics line up with Rust&apos;s ownership rules almost exactly:</p>
    <ul className="plain-list">
      <li>
        A qubit can&apos;t be copied (the no-cloning theorem), so <code>Qubit</code> has no <code>Clone</code> or{' '}
        <code>Copy</code>.
      </li>
      <li>
        Measuring destroys the quantum state, so <code>measure(q)</code> takes the qubit by value and returns a
        classical bit.
      </li>
      <li>
        A two-qubit gate needs two different qubits, so <code>cx(&amp;mut a, &amp;mut b)</code> borrows both
        mutably, and the borrow checker rejects passing the same qubit twice.
      </li>
    </ul>
    <p>
      Code that breaks these rules is not just wrong at run time. It does not compile, and the unmodified Rust
      compiler is the thing that says so. Qxide has no separate type checker for this.
    </p>
    <CodeBlock
      label="Entangling two qubits"
      lang="rust"
      code={`let mut c = Circuit::new();
let mut a = c.qubit();
let mut b = c.qubit();
c.h(&mut a);
c.cx(&mut a, &mut b);
let ma = c.measure(a); // a is gone; only a classical bit remains`}
    />

    <h2>Honest about what is not new</h2>
    <p>
      Treating qubits as values you can&apos;t copy is not Qxide&apos;s invention. Guppy and Silq already do it. Rust
      gives <em>affine</em> types: a qubit can&apos;t be used twice, but dropping one without measuring it is not
      forbidden. Guppy is stricter, because it also rejects discarding a qubit. The README has a{' '}
      <a href={`${GITHUB_BLOB}/README.md`} target="_blank" rel="noreferrer">
        comparison table
      </a>{' '}
      with Guppy, Silq, Q#, Qiskit and Cirq, with sources.
    </p>

    <h2>What works today</h2>
    <p>
      Today, Qxide is a Rust crate and a small command-line tool (stage 0 of the roadmap). It includes:
    </p>
    <ul className="plain-list">
      <li>
        gates <code>h</code>, <code>x</code>, <code>z</code>, <code>s</code>, <code>t</code>, <code>rx</code>,{' '}
        <code>ry</code>, <code>rz</code>, <code>cx</code> and <code>ccx</code>, plus measurement and reset;
      </li>
      <li>
        branching on measured bits with <code>if_one</code> and <code>if_zero</code>;
      </li>
      <li>
        <code>with_ancilla</code>, which computes into a helper qubit and automatically appends the inverse so the
        helper returns to 0;
      </li>
      <li>OpenQASM 3 output;</li>
      <li>
        a state-vector simulator and a stabilizer simulator on the CPU, chosen automatically, and a GPU prototype
        behind the <code>gpu</code> feature.
      </li>
    </ul>
    <p>
      Qxide cannot yet submit jobs to real quantum hardware. You export OpenQASM 3 and use another tool for that.
    </p>

    <h2>Plan: the .qx language</h2>
    <p>
      The library is a way to test the idea before designing syntax. The{' '}
      <a href={`${GITHUB_BLOB}/ROADMAP.md`} target="_blank" rel="noreferrer">
        roadmap
      </a>{' '}
      lists what comes next. <strong>None of the following exists yet.</strong>
    </p>
    <ul className="plain-list">
      <li>
        <strong>Stage 1, .qx syntax (planned):</strong> a syntax draft informed by what felt awkward in the library,
        a parser for <code>.qx</code> files, a <code>qxide build hello.qx</code> command that produces OpenQASM 3, and
        VS Code syntax highlighting.
      </li>
      <li>
        <strong>Stage 2, compiler (planned):</strong> compiling the classical parts to Rust or native code, sending
        the quantum parts to quantum computing services as OpenQASM 3, and a package manager (<code>Qx.toml</code>).
      </li>
    </ul>
    <p>
      The <Link to="/playground">playground</Link> is also planned and will arrive with the language. In the
      meantime, the <Link to="/learn">tutorial</Link> shows everything that works now.
    </p>
  </>
);

// Newest first.
export const POSTS: Post[] = [
  {
    slug: 'qxide-0-7-0-modules-and-packages',
    title: 'Qxide 0.7.0: modules and packages',
    date: '2026-11-21',
    summary: 'Qxide 0.7.0 adds modules and packages, four new CLI commands, and independent per-shot sampling.',
    body: release070,
  },
  {
    slug: 'qxide-0-6-0-functions-with-qubits',
    title: 'Qxide 0.6.0: functions with qubits',
    date: '2026-11-20',
    summary: 'Qxide 0.6.0 adds functions that borrow, consume, and return qubits, conditional gate-only calls on measured bits, var type inference, numeric casts, and PI.',
    body: release060,
  },
  {
    slug: 'qxide-0-5-0-the-qx-language',
    title: 'Qxide 0.5.0: the .qx language',
    date: '2026-11-20',
    summary:
      'Qxide 0.5.0 introduces the .qx language with Java-style syntax, compile-time qubit safety, and qxide CLI subcommands run, check, and qasm.',
    body: install050,
  },
  {
    slug: 'qxide-0-4-1-install-with-one-line',
    title: 'Qxide 0.4.1: install with one line',
    date: '2026-11-20',
    summary:
      'The qxide executable, install scripts for macOS, Linux and Windows, and the CLI on crates.io and PyPI.',
    body: install041,
  },
  {
    slug: 'why-qxide',
    title: 'Why Qxide',
    date: '2026-11-19',
    summary:
      'Why we are building a language for classical and quantum computers, what already works, and what is only planned.',
    body: whyQxide,
  },
];

export function formatDate(iso: string): string {
  return new Date(`${iso}T00:00:00Z`).toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    timeZone: 'UTC',
  });
}
