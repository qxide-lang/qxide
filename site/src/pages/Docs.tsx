import {Link} from 'react-router';
import {CodeBlock} from '../components/CodeBlock';
import {GITHUB, GITHUB_BLOB, usePageMeta} from '../lib/meta';
import {Inline} from '../components/Inline';

const HELP = `Usage: qxide <COMMAND>

Commands:
  run [FILE] [--shots N] [--seed S]  Run a .qx program and simulate it (default: 1 shot, random seed, printed to stderr)
  run [FILE] --backend ibm:DEVICE    Run a .qx program on an IBM quantum computer (prompts before submitting; uses QPU time)
                                     --dry-run (preview), --yes (no prompt), --max-seconds S (QPU cap, default 120), --device-config FILE (dry-run)
  check [FILE]                       Check a .qx program for errors without running it
  qasm [FILE]                        Print the OpenQASM 3 circuit built by a .qx program
  fmt <FILE> [--check]               Format a .qx file in place (--check exits non-zero if it would change)
  repl [--seed S]                    Start an interactive .qx session (default: random seed, printed to stderr)
  explain <CODE>                     Explain an error code with an example and fix (e.g. Q0302)
  new <NAME>                         Create a new project folder with main.qx and README.md
  bell [SHOTS]                       Print the Bell-state circuit as OpenQASM 3 and simulate it (default: 1000 shots)
  lsp                                Start a .qx language server (LSP) on stdin/stdout
  add <PKG>[@VER] [--path DIR]       Add a dependency to qxide.toml and install it
  remove <PKG>                       Remove a dependency
  install [--locked] [--offline]     Download and verify packages in qxide.lock
  publish [--dry-run]                Package the project and submit it to the registry
  search <WORD>                      Search the package registry
  app new <NAME>                   Create a desktop app project with .qx logic and an HTML/JS UI
  app dev [DIR]                    Run the app in a window, loading files directly from disk
  app build [DIR]                  Build a portable app executable in target/app/

Options:
  --engine ast|ir|jit                Select engine for run, qasm, and repl (default: QXIDE_ENGINE or ast; ir: typed IR; jit: IR + native, needs jit feature)
  -V, --version                      Print version
  -h, --help                         Print help`;

const REPO_DOCS = [
  {
    path: 'docs/design/qx-language.md',
    title: '.qx language design',
    desc: 'Goals, syntax, the linear qubit type system, diagnostics, the 0.5.0 Java-style syntax revision, qubit parameters and returns, conditional calls, var, casts and PI (sections 13 and 14), and the native-code roadmap.',
    lang: 'Korean',
  },
  {
    path: 'docs/design/qx-packages.md',
    title: 'Modules and packages',
    desc: 'Design document covering .qx modules, import statements, qxide.toml, qxide.lock, the qxide-lang/registry repository, and the package commands.',
    lang: 'English',
  },
  {
    path: 'docs/error-codes.md',
    title: 'Error code reference',
    desc: 'Catalog of every .qx error code with explanations, examples, and fixes, also printed by qxide explain.',
    lang: 'English',
  },
  {
    path: 'docs/design/qx-ir.md',
    title: 'Typed IR design',
    desc: "Design of the typed IR that checked `.qx` programs lower to, implemented in 0.8.0 as an IR interpreter selected with `--engine ir` (the tree-walking `ast` engine remains the default).",
    lang: 'Korean',
  },
  {
    path: 'docs/design/qx-jit.md',
    title: 'Cranelift JIT',
    desc: "Design record of the optional Cranelift JIT (`--engine jit`, Cargo feature `jit`, off by default), which compiles build-time classical functions using only `int`, `double`, and `boolean` to native code and runs everything else in the IR interpreter.",
    lang: 'Korean',
  },
  {
    path: 'docs/design/qx-qpu.md',
    title: 'Quantum computer submission (IBM)',
    desc: "Design record of `qxide run --backend ibm:<device>`: OpenQASM 3 checks, IBM Quantum (Qiskit Runtime REST) client, credentials, conversion to native gates and coupled qubits, cost and queue notices, and what is unverified against the real service (tested only against a local fake server).",
    lang: 'Korean',
  },
  {
    path: 'docs/perf/QX_BENCHMARKS.md',
    title: '.qx benchmark suite',
    desc: "The `.qx` benchmark suite in `bench/qx/`, how to run it, the measurement method, and measured times for the `ast`, `ir`, and `jit` engines on one Windows laptop.",
    lang: 'English',
  },
  {
    path: 'docs/design/lsp.md',
    title: 'Language server and VS Code extension',
    desc: "Design of the `qxide lsp` language server (diagnostics, formatting, hover, symbols, go to definition, and completions) and the VS Code extension in `editors/vscode`.",
    lang: 'English',
  },
  {
    path: 'docs/design/qx-app.md',
    title: 'Desktop apps and Qxide Studio',
    desc: "Design of desktop apps written in `.qx` (`qxide app new|dev|build`, the `qxide-app` webview runtime, permissions, and the `std` modules) and Qxide Studio.",
    lang: 'English',
  },
  {
    path: 'CHANGELOG.md',
    title: 'Change log',
    desc: 'Per-version list of notable changes across releases, starting with 0.6.0.',
    lang: 'English',
  },
  {
    path: 'docs/release-notes/v0.8.0.md',
    title: 'v0.8.0 release notes',
    desc: "What changed in 0.8.0: the typed IR engine and optional Cranelift JIT, IBM quantum computer submission, the `String` type and `std` library, desktop apps and Qxide Studio, the language server and VS Code extension, registry package owners, a random default seed, execution budgets, and breaking changes.",
    lang: 'English',
  },
  {
    path: 'docs/release-notes/v0.7.0.md',
    title: 'v0.7.0 release notes',
    desc: 'What changed in 0.7.0: .qx modules and public imports, the package manager and qxide-lang/registry, new CLI commands (qxide new, qxide fmt, qxide repl, qxide explain), independent per-shot seeding, and breaking keyword, linearity, nesting, and non-finite angle checks.',
    lang: 'English',
  },
  {
    path: 'docs/release-notes/v0.6.0.md',
    title: 'v0.6.0 release notes',
    desc: 'What changed in 0.6.0: functions with qubit parameters and returns, conditional calls, var, casts, PI, breaking .qx keyword changes, and English Rust library docs.',
    lang: 'English',
  },
  {
    path: 'docs/release-notes/v0.5.0.md',
    title: 'v0.5.0 release notes',
    desc: 'What changed in 0.5.0: the .qx language and CLI, Java-style syntax, bit output and diagnostics.',
    lang: 'English',
  },
  {
    path: 'docs/release.md',
    title: 'Release runbook',
    desc: 'How releases are cut: cargo-dist installers and archives on GitHub Releases, and PyPI wheels built with maturin.',
    lang: 'English',
  },
  {
    path: 'docs/vscode-extension.md',
    title: 'VS Code extension releases',
    desc: "How the VS Code extension is packaged as a `.vsix`, attached to GitHub releases, and published to Visual Studio Marketplace and Open VSX.",
    lang: 'English',
  },
  {
    path: 'docs/gpu.md',
    title: 'GPU backend',
    desc: 'The gpu feature: how to enable it, the CubeCL kernels, and how the benchmark was measured.',
    lang: 'Korean',
  },
  {
    path: 'docs/design/uncomputation.md',
    title: 'Automatic uncomputation',
    desc: 'The design behind with_ancilla: scope-based reversal, control-only access and runtime guards.',
    lang: 'Korean',
  },
  {
    path: 'docs/design/entanglement-tracking.md',
    title: 'Entanglement tracking',
    desc: 'How the library tracks which qubits may be entangled, and the alternatives that were considered.',
    lang: 'Korean',
  },
  {
    path: 'README.md',
    title: 'README',
    desc: 'Overview, install options, a .qx quick start with real output, and the Rust library.',
    lang: 'English',
  },
  {
    path: 'README.ko.md',
    title: 'README (Korean)',
    desc: 'The Korean translation of the README.',
    lang: 'Korean',
  },
  {
    path: 'ROADMAP.md',
    title: 'Roadmap',
    desc: "Stage 0 (Rust library), stage 1 (`.qx` syntax), stage 2 (compiler, current: typed IR and Cranelift JIT done; LLVM backend and verified quantum hardware submission open), and stage 3 (desktop apps and self-hosting); unchecked items are not built yet.",
    lang: 'Korean',
  },
];

export function Docs() {
  usePageMeta(
    'Qxide documentation: API, design notes and CLI',
    'Where to find Qxide documentation: the API reference on docs.rs, design notes and runbooks in the repository, and a summary of the qxide command.',
  );

  return (
    <article className="prose">
      <header className="page-head">
        <h1>Docs</h1>
        <p className="lede">
          <Inline text={"Qxide is an early experiment. Documentation is split between the Rust API reference on docs.rs and documents in the GitHub repository, including design records for the typed IR, Cranelift JIT, and IBM quantum computer submission, plus the package manager design, benchmark results, and every error code."} /> New here? Start with the <Link to="/learn">tutorial</Link>.
        </p>
      </header>

      <section aria-labelledby="api">
        <h2 id="api">Rust API reference</h2>
        <p>
          The <code>.qx</code> language is documented by the tutorial and the design document below. Every public type
          and method of the <code>qxide</code> Rust crate (<code>Circuit</code>, <code>Qubit</code>,{' '}
          <code>Backend</code> and the rest) is documented on docs.rs.
        </p>
        <p>
          <a className="pill pill-solid" href="https://docs.rs/qxide" target="_blank" rel="noreferrer">
            Open docs.rs/qxide<span className="sr-only"> (opens in a new tab)</span>
          </a>
        </p>
      </section>

      <section aria-labelledby="repo-docs">
        <h2 id="repo-docs">Documents in the repository</h2>
        <p>
          These live in the{' '}
          <a href={GITHUB} target="_blank" rel="noreferrer">
            GitHub repository
          </a>
          . Some are written in Korean; the language is noted on each entry.
        </p>
        <ul className="doc-list">
          {REPO_DOCS.map((d) => (
            <li key={d.path}>
              <a href={`${GITHUB_BLOB}/${d.path}`} target="_blank" rel="noreferrer">
                {d.title}
                <span className="sr-only"> (opens in a new tab)</span>
              </a>
              <span className="doc-meta">
                <code>{d.path}</code> · {d.lang}
              </span>
              <span className="doc-desc">
                <Inline text={d.desc} />
              </span>
            </li>
          ))}
        </ul>
      </section>

      <section aria-labelledby="cli">
        <h2 id="cli">Command line</h2>
        <p>
          The <code>qxide</code> binary runs, checks, exports, and formats <code>.qx</code> programs, starts an interactive session, explains errors, and manages packages. This is the output of <code>qxide --help</code>:
        </p>
        <CodeBlock label="qxide --help" code={HELP} />
        <ul className="plain-list">
          <li>
            <code>qxide run [FILE]</code>: <Inline text={"Checks and simulates a program, defaulting to `src/main.qx` in a project when no file is given. `--shots N` prints a table grouped by outcome (such as `[0, 1, 1]  0b110 = 6  ×503`). Without `--seed`, runs use a random seed and print `seed: S (pass --seed S to reproduce)` to stderr if output depends on measurements. `--seed S` makes runs repeatable, deriving each shot's seed from `S`."} />
          </li>
          <li>
            <code>--engine ast|ir|jit</code>: <Inline text={"Selects the circuit-building engine for `qxide run`, `qxide qasm`, and `qxide repl`. The default is `ast` (the tree-walking interpreter), or the `QXIDE_ENGINE` environment variable if set. `ir` runs the typed IR interpreter with the same output. `jit` also compiles build-time classical functions to native code with Cranelift. It requires a build with the Cargo feature `jit`, which prebuilt binaries and the PyPI package do not include; other builds reject it with exit code 2."} />
          </li>
          <li>
            <code>qxide run [FILE] --backend ibm:DEVICE</code>: <Inline text={"Sends the circuit to an IBM Quantum computer instead of simulating it. `--dry-run` prints the request without reading credentials or using the network. A real run asks for confirmation (unless `--yes` is passed), uses paid or quota-limited QPU time, and caps it with `--max-seconds` (default 120). This path has been tested only against a local fake server so far. Details are in the"} />{' '}
            <Link to="/learn#ibm">tutorial</Link>.
          </li>
          <li>
            <code>qxide check [FILE]</code> parses, type-checks, and verifies qubit usage without running the circuit, while <code>qxide qasm [FILE]</code> prints the circuit as OpenQASM 3. Inside a project, both default to <code>src/main.qx</code> when no file is given.
          </li>
          <li>
            <code>qxide fmt FILE</code> formats a file in place while keeping comments (add <code>--check</code> to verify formatting without editing), and <code>qxide repl</code> starts an interactive session under the same rules as <code>qxide run</code>. <code>qxide explain CODE</code> prints the explanation, an example, and the fix for an error code from <code>docs/error-codes.md</code>.
          </li>
          <li>
            <code>qxide new NAME</code> creates a project folder containing <code>qxide.toml</code>, a starter Bell-pair program in <code>src/main.qx</code>, <code>README.md</code>, and <code>.gitignore</code>.
          </li>
          <li>
            <code>qxide add</code>, <code>qxide remove</code>, and <code>qxide install</code> (with <code>--locked</code> and <code>--offline</code> options) manage dependencies in <code>qxide.toml</code> and record their SHA-256 checksums in <code>qxide.lock</code>. <code>qxide search</code> queries the GitHub registry <code>qxide-lang/registry</code>, and <code>qxide publish</code> packages a project and opens a pull request there using the GitHub CLI <code>gh</code>.
          </li>
          <li>
            <code>qxide lsp</code>: <Inline text={"Starts a language server on stdin/stdout for diagnostics, formatting, hover, symbols, go to definition, and completions. The VS Code extension (a `.vsix` attached to the GitHub release; Marketplace listing in preparation) starts it automatically and requires `qxide` 0.8.0 or newer and VS Code 1.91 or newer."} />
          </li>
          <li>
            <code>qxide app new|dev|build</code>: <Inline text={"Creates, runs, and packages desktop apps with `.qx` logic and an HTML/JS UI. `qxide app build` attaches the app to a prebuilt `qxide-app` runtime. That runtime is not yet included in release archives or PyPI wheels and must be built from `crates/qxide-app-runtime` in the repository, as described in `docs/design/qx-app.md`."} />
          </li>
          <li>
            <code>qxide bell</code> prints the OpenQASM 3 for a built-in Bell-state circuit, followed by outcome counts from 1000 simulated shots. Run <code>qxide --version</code> to print the installed version.
          </li>
        </ul>
        <p>
          Install instructions are on the <Link to="/learn#install">Learn page</Link>.
        </p>
      </section>
    </article>
  );
}
