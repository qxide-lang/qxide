import {Link} from 'react-router';
import {CodeBlock} from '../components/CodeBlock';
import {GITHUB, GITHUB_BLOB, usePageMeta} from '../lib/meta';

const HELP = `Usage: qxide <COMMAND>

Commands:
  run <FILE> [--shots N] [--seed S]  Run a .qx program and simulate it (default: 1 shot, seed 0)
  check <FILE>                       Check a .qx program for errors without running it
  qasm <FILE>                        Print the OpenQASM 3 circuit built by a .qx program
  bell [SHOTS]                       Print the Bell-state circuit as OpenQASM 3 and simulate it (default: 1000 shots)

Options:
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
    path: 'docs/design/qx-ir.md',
    title: 'Typed IR design draft',
    desc: 'Design draft of the typed IR planned to replace the tree-walking interpreter and prepare for native code generation; not implemented yet.',
    lang: 'Korean',
  },
  {
    path: 'CHANGELOG.md',
    title: 'Change log',
    desc: 'Per-version list of notable changes across releases, starting with 0.6.0.',
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
    desc: 'Stage 0 (Rust library, current), stage 1 (.qx syntax) and stage 2 (compiler). Unchecked items are not built yet.',
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
          Qxide is an early experiment, so the docs are split between the Rust API reference and a handful of documents
          in the repository. New here? Start with the <Link to="/learn">tutorial</Link>.
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
              <span className="doc-desc">{d.desc}</span>
            </li>
          ))}
        </ul>
      </section>

      <section aria-labelledby="cli">
        <h2 id="cli">Command line</h2>
        <p>
          The <code>qxide</code> binary runs, checks and exports <code>.qx</code> programs. This is the output of{' '}
          <code>qxide --help</code>:
        </p>
        <CodeBlock label="qxide --help" code={HELP} />
        <ul className="plain-list">
          <li>
            <code>qxide run FILE</code> checks and simulates a program. <code>--shots N</code> repeats it and prints a
            table grouped by outcome, such as <code>[0, 1, 1]  0b110 = 6  ×503</code>; <code>--seed S</code> makes a
            run repeatable.
          </li>
          <li>
            <code>qxide check FILE</code> parses and type-checks a program and verifies qubit usage without running
            it.
          </li>
          <li>
            <code>qxide qasm FILE</code> prints the circuit as OpenQASM 3.
          </li>
          <li>
            <code>qxide bell</code> prints the OpenQASM 3 for a built-in Bell-state circuit, then the counts from 1000
            simulated shots. <code>qxide --version</code> prints the installed version.
          </li>
        </ul>
        <p>
          Install instructions are on the <Link to="/learn#install">Learn page</Link>.
        </p>
      </section>
    </article>
  );
}
