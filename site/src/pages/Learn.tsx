import {Link} from 'react-router';
import {CodeBlock} from '../components/CodeBlock';
import {GITHUB, GITHUB_BLOB, GITHUB_TREE, usePageMeta} from '../lib/meta';
import {Inline} from '../components/Inline';

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

const BELL_RUN = `$ qxide run examples/qx/bell.qx --seed 0
a=1 b=1`;

const BELL_SHOTS = `$ qxide run examples/qx/bell.qx --shots 1000 --seed 0
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
    println("{}  {} = {}", m, m.toBinary(), m.asInt());
}`;

const GHZ_OUT = `$ qxide run examples/qx/ghz.qx --shots 1000 --seed 0
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

const TELEPORT_OUT = `$ qxide run examples/qx/teleport.qx --shots 1000 --seed 0
[1, 0, 1]  0b101 = 5  ×273
  bob=1
[0, 0, 1]  0b100 = 4  ×251
  bob=1
[1, 1, 1]  0b111 = 7  ×248
  bob=1
[0, 1, 1]  0b110 = 6  ×228
  bob=1`;

const PKG_SETUP = `$ qxide new demo
Created project \`demo\` in \`demo\` with \`qxide.toml\`, \`src/main.qx\`, \`README.md\`, and \`.gitignore\`
Next:
  qxide run demo/src/main.qx
  qxide fmt demo/src/main.qx
$ cd demo
$ qxide add bell-utils
       Added bell-utils v0.1.0
      Locked 1 packages
  Downloaded bell-utils v0.1.0`;

const PKG_MAIN = `import bell_utils.bell_pair;
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

const PKG_RUN = `$ qxide run --shots 1000 --seed 0
[1, 1, 0]  0b011 = 3  ×273
  a=1 b=1 plus=0
[0, 0, 0]  0b000 = 0  ×251
  a=0 b=0 plus=0
[1, 1, 1]  0b111 = 7  ×248
  a=1 b=1 plus=1
[0, 0, 1]  0b100 = 4  ×228
  a=0 b=0 plus=1`;

// Captured with qxide 0.8.0 (dry run: no credentials read, nothing sent).
const IBM_DRY = `$ qxide run examples/qx/bell.qx --backend ibm:ibm_torino --shots 1000 --dry-run
Dry run (nothing sent, no credentials read); job request for ibm:ibm_torino (1000 shots):

POST https://quantum.cloud.ibm.com/api/v1/jobs
Authorization: Bearer <IAM token from API key in QXIDE_IBM_API_KEY>
Service-CRN: <instance CRN from QXIDE_IBM_INSTANCE>
IBM-API-Version: 2026-04-15
Content-Type: application/json

{
  "backend": "ibm_torino",
  "cost": 120,
  "params": {
    "options": {
      "default_shots": 1000
    },
    "pubs": [
      [
        "OPENQASM 3.0;\\ninclude \\"stdgates.inc\\";\\nqubit[2] q;\\nbit[2] c;\\nh q[0];\\ncx q[0], q[1];\\nc[0] = measure q[0];\\nc[1] = measure q[1];\\n",
        null,
        1000
      ]
    ],
    "support_qiskit": true,
    "version": 2
  },
  "program_id": "sampler"
}

OpenQASM 3:
OPENQASM 3.0;
include "stdgates.inc";
qubit[2] q;
bit[2] c;
h q[0];
cx q[0], q[1];
c[0] = measure q[0];
c[1] = measure q[1];

Note: the circuit above is shown before conversion; a real run downloads the device configuration and rewrites it to the device's native gates and physical qubits.
Pass --device-config with a saved configuration JSON to see the exact circuit.`;

const ENGINE_IR = `$ qxide run examples/qx/bell.qx --shots 1000 --seed 0 --engine ir
[1, 1]  0b11 = 3  ×521
  a=1 b=1
[0, 0]  0b00 = 0  ×479
  a=0 b=0`;

const ENGINE_JIT = `$ qxide run examples/qx/bell.qx --engine jit
qxide: \`jit\` not available for --engine (this build has no JIT; rebuild with \`cargo build --features jit\`, or use \`--engine ir\`)`;

// Minimum elaborate ms from the Cranelift JIT table in docs/perf/QX_BENCHMARKS.md (one run, same machine state for all engines).
const BENCH = `Case            What it does                          ast ms   ir ms   jit ms
arith           prime counting, pi series              813.2   542.9     83.7
recursion       fib(24), mutual recursion              179.1    56.9      5.4
loops           300 x 300 nested loop in main          253.0    73.8     70.0
circuit_build   about 300,000 gates via a helper       155.3    74.8     74.8
qft             12-qubit QFT (simulation dominates)      0.5     0.3      1.3
run_flat_8000   8000 functions, each called once        11.1    44.1     43.1`;

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
  ['ibm', "Run on an IBM quantum computer"],
  ['packages', 'Projects and packages'],
  ['engines', "Execution engines"],
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
          <Inline text={"A short tutorial for people new to quantum computing. Every snippet is a `.qx` program (or Rust code, where marked) that exists today, and every output shown was captured by running it with Qxide 0.8.0. Commands pass `--seed 0` so you get the same output."} />
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
          <Inline text={"Check that it worked with `qxide --version`. `pip install qxide` installs the same command-line tool. Since crates.io still has 0.7.0, `cargo install qxide` installs 0.7.0 until 0.8.0 is published there. The example programs below are in the repository's"} />{' '}
          <a href={`${GITHUB_TREE}/examples/qx`} target="_blank" rel="noreferrer">
            examples/qx
          </a>{' '}
          <Inline text={"folder; save a snippet to a `.qx` file to run it."} />
        </p>
        <p>
          <Inline text={"For editor support, download `qxide-vscode-0.1.0.vsix` from the v0.8.0 GitHub release and run `code --install-extension qxide-vscode-0.1.0.vsix`. The VS Code extension adds highlighting and starts `qxide lsp` for errors as you type, hover, formatting, and completions. It needs `qxide` 0.8.0 or newer on your `PATH` (or set `qxide.path`) and VS Code 1.91 or newer. A Visual Studio Marketplace listing is in preparation."} />{' '}
          <a href={`${GITHUB}/releases/tag/v0.8.0`} target="_blank" rel="noreferrer">
            Open the v0.8.0 release<span className="sr-only"> (opens in a new tab)</span>
          </a>
          .
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
          That is the entanglement: each bit alone is a coin flip, but they always agree.{' '}
          <Inline text={"Without `--seed`, each run picks a random seed and prints a line like `seed: 452185838572849581 (pass --seed 452185838572849581 to reproduce)` to stderr, so your counts will differ from the ones here. Pass `--seed 0`, as the commands on this page do, to get these exact numbers."} />
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
          Every run is all zeros or all ones, never a mix. <code>m.toBinary()</code> prints the register with the
          highest bit first and <code>m.asInt()</code> turns it into a number, so all ones reads <code>0b11111</code>{' '}
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
          gate. <Inline text={"Errors include codes (such as `Q0301`) and `note` or `help` lines. Run `qxide explain Q0301` to print an explanation, an example, and a fix for any code."} />
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
          <Inline text={"CI parses the OpenQASM 3 from all seven `examples/qx` programs and ten Rust sample circuits with the reference parser (`openqasm3` 1.0.2, the grammar published with the specification) and imports it with Qiskit 2.6.2 (`qiskit.qasm3.loads`). It also converts each example to the native gates of three offline IBM device snapshots and checks the result with the official IBM validator, without network access. You can paste the text into other tools, or send it to IBM hardware from Qxide, as the next section shows."} />
        </p>
      </section>

      <section aria-labelledby="ibm">
        <h2 id="ibm">Run on an IBM quantum computer</h2>
        <p>
          <Inline text={"`qxide run` can send the circuit to an IBM Quantum computer instead of the simulator: `qxide run FILE --backend ibm:DEVICE --shots N`, where `DEVICE` is a device name such as `ibm_torino` available to your IBM Quantum account. Results return in the same format as `--shots`. Start with `--dry-run`, which reads no credentials and sends nothing; it prints the job request and the circuit."} />
        </p>
        <CodeBlock label="Dry run for an IBM device" copyable={false} code={IBM_DRY} />
        <p>
          <Inline text={"The circuit shown is the logical circuit. A real run downloads the device configuration and rewrites the circuit to the device's native gates and physical qubits. To inspect that rewritten circuit offline, pass a saved configuration JSON file with `--device-config FILE`."} />
        </p>
        <p>
          <Inline text={"A real run reads the API key from the `QXIDE_IBM_API_KEY` environment variable or, if it is not set, from the operating system's credential store (Windows Credential Manager entry `qxide:ibm-quantum`, macOS keychain service `qxide` account `ibm-quantum`, or Secret Service on Linux). Qxide never writes to the store and has no flag for the key. The instance CRN comes from `QXIDE_IBM_INSTANCE`. Secrets are passed to the system `curl` on stdin, not on the command line."} />
        </p>
        <p>
          <Inline text={"This uses real QPU time. Paid plans are billed and the free Open plan has a small monthly allowance; check your plan on IBM Quantum Platform, as Qxide does not show prices. Before submitting, Qxide prints the shots, the device's queue length, and the time limit, then prompts `[y/N]` (pass `--yes` to skip; without a terminal and without `--yes`, it refuses). `--max-seconds S` (default 120, at most 10800) asks the service to stop the job after that much QPU time. Results can take minutes to hours. `Ctrl-C` only stops waiting; the job stays queued or running and keeps using QPU time. Check or cancel it on IBM Quantum Platform using the printed job ID."} />
        </p>
        <p>
          <Inline text={"This path has been tested only against a local fake server, and no job has been sent to the real IBM service yet. Treat a first real run with care and report problems on GitHub. `--seed` and `--allow` cannot be combined with `--backend`. Programs without measurements, or with file or window effects, are refused."} /> Details are in{' '}
          <a href={`${GITHUB_BLOB}/docs/design/qx-qpu.md`} target="_blank" rel="noreferrer">
            docs/design/qx-qpu.md
          </a>{' '}
          (Korean).
        </p>
      </section>

      <section aria-labelledby="packages">
        <h2 id="packages">Projects and packages</h2>
        <p>Running <code>qxide new</code> creates a project directory containing <code>qxide.toml</code>, a starter <code>src/main.qx</code>, <code>README.md</code>, and <code>.gitignore</code>. You can then run <code>qxide add</code> to download a package from the registry, record it in <code>qxide.toml</code>, and pin its SHA-256 checksum in <code>qxide.lock</code>.</p>
        <CodeBlock label="New project and a package" copyable={false} code={PKG_SETUP} />
        <p>Functions marked <code>public</code> can be brought into scope with <code>import</code>, using <code>_</code> in place of <code>-</code> for the package name so <code>bell-utils</code> becomes <code>bell_utils</code>. Qxide's qubit rules apply across package boundaries, so your code owns any qubits returned by an imported function and must still measure them.</p>
        <CodeBlock label="src/main.qx" lang="qx" code={PKG_MAIN} />
        <CodeBlock label="1000 shots" copyable={false} code={PKG_RUN} />
        <p>In the output, <code>a</code> and <code>b</code> always agree because <code>bell_pair</code> entangles them, while the qubit from <code>plus</code> acts as an independent coin flip. To reproduce the exact same dependency set on another machine, run <code>qxide install --locked</code>, or use <code>qxide publish</code> to share your own package by opening a pull request to <code>qxide-lang/registry</code>.</p>
        <p>Qxide also includes a few commands for everyday work inside or outside a project. <code>qxide fmt</code> formats a <code>.qx</code> file in place while preserving comments, and <code>qxide repl</code> opens an interactive session where you can try statements one at a time under the same qubit rules as <code>qxide run</code>. If the compiler reports an error code such as <code>Q0301</code>, running <code>qxide explain</code> with that code prints an explanation, an example, and a fix.</p>
      </section>

      <section aria-labelledby="engines">
        <h2 id="engines">Execution engines</h2>
        <p>
          <Inline text={"A `.qx` program first runs once on your computer to build the circuit (loops, function calls, and classical arithmetic happen here), and then the circuit is simulated. `--engine` picks what runs that first step for `qxide run`, `qxide qasm`, and `qxide repl`: `ast` (the default) walks the syntax tree, `ir` runs a typed intermediate representation, and `jit` also compiles build-time classical functions using only `int`, `double`, and `boolean` to native code with Cranelift. All engines give the same output. The `QXIDE_ENGINE` environment variable sets the default."} />
        </p>
        <CodeBlock label="The ir engine" copyable={false} code={ENGINE_IR} />
        <p>
          <Inline text={"`jit` is behind the Cargo feature `jit`, which is off by default and not included in the prebuilt binaries, the install scripts, or the PyPI package. Those builds reject `--engine jit` with exit code 2, as shown below. To enable it, build from a clone of the repository with `cargo install --path . --features jit`. The browser playground always uses `ast` and has no JIT."} />
        </p>
        <CodeBlock label="jit in a build without the jit feature" copyable={false} code={ENGINE_JIT} />
        <p>
          <Inline text={"The numbers below come from the benchmark suite in `bench/qx/` (`docs/perf/QX_BENCHMARKS.md`). They are from one run on a single Windows 11 laptop (Intel i9-14900HX) busy with other work (about 49% CPU load), using a release build with 7 repetitions timed inside the process. Each value is the fastest of the 7 runs of the circuit-building step in milliseconds; lower is better."} />
        </p>
        <CodeBlock label="Circuit-building time, fastest of 7 runs" copyable={false} code={BENCH} />
        <p>
          <Inline text={"The JIT helps when the work is in classical functions: `arith` (prime counting) and `recursion` (`fib`). `loops` benefits from `ir` but not `jit`, because its loops sit in `main`, which prints and is never compiled. `circuit_build` applies gates in its helper, so nothing is compiled there either. For `qft` (state-vector simulation), building takes under 2 ms on every engine, and total time is dominated by simulation, which no engine changes. Programs that run each function once can be slower with `ir` and `jit`, because every function is converted first. These are numbers from one busy machine, not a general speed claim, and `ast` remains the default engine."} /> The full tables, with medians and every phase, are in{' '}
          <a href={`${GITHUB_BLOB}/docs/perf/QX_BENCHMARKS.md`} target="_blank" rel="noreferrer">
            docs/perf/QX_BENCHMARKS.md
          </a>
          .
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
            <a href={`${GITHUB_BLOB}/docs/release-notes/v0.8.0.md`} target="_blank" rel="noreferrer">
              v0.8.0 release notes
            </a>
            : <Inline text={"What changed in 0.8.0: the typed IR engine and optional Cranelift JIT, IBM quantum computer submission, the `String` type and `std` library, desktop apps and Qxide Studio, the language server and VS Code extension, registry package owners, a random default seed, execution budgets, and breaking changes."} />
          </li>
          <li>
            <Link to="/blog/qxide-0-7-0-modules-and-packages">Qxide 0.7.0</Link>: modules and packages, <code>qxide new</code>, <code>qxide fmt</code>, <code>qxide repl</code>, <code>qxide explain</code>, and independent shots.
          </li>
        </ul>
      </section>
    </article>
  );
}
