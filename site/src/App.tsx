import {useEffect, useState} from 'react';

const GITHUB = 'https://github.com/qxide-lang/qxide';

// TODO: Learn, Playground and Blog have no pages yet; point them at real pages when they exist.
const NAV: {label: string; href: string; external?: boolean}[] = [
  {label: 'Learn', href: '#'},
  {label: 'Docs', href: `${GITHUB}/tree/main/docs`, external: true},
  {label: 'Playground', href: '#'},
  {label: 'Examples', href: `${GITHUB}/tree/main/examples`, external: true},
  {label: 'Blog', href: '#'},
  {label: 'GitHub', href: GITHUB, external: true},
];

type Os = 'unix' | 'windows';

// TODO: https://qxide.dev/install.sh and /install.ps1 are not published yet.
const INSTALL: Record<Os, {tab: string; prompt: string; cmd: string}> = {
  unix: {tab: 'macOS / Linux', prompt: '$', cmd: 'curl -fsSL https://qxide.dev/install.sh | sh'},
  windows: {tab: 'Windows', prompt: 'PS>', cmd: 'irm https://qxide.dev/install.ps1 | iex'},
};

function detectOs(): Os {
  if (typeof navigator === 'undefined') return 'unix';
  const nav = navigator as Navigator & {userAgentData?: {platform?: string}};
  const platform = nav.userAgentData?.platform ?? navigator.platform ?? navigator.userAgent;
  return /win/i.test(platform) ? 'windows' : 'unix';
}

function Install() {
  const [os, setOs] = useState<Os>('unix');
  const [copied, setCopied] = useState(false);

  useEffect(() => setOs(detectOs()), []);

  const current = INSTALL[os];

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(current.cmd);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      setCopied(false);
    }
  };

  return (
    <section className="install" aria-label="Install Qxide">
      <div className="tabs" role="tablist" aria-label="Operating system">
        {(Object.keys(INSTALL) as Os[]).map((key) => (
          <button
            key={key}
            type="button"
            role="tab"
            id={`tab-${key}`}
            aria-selected={os === key}
            aria-controls="install-panel"
            className="tab"
            onClick={() => setOs(key)}
          >
            {INSTALL[key].tab}
          </button>
        ))}
      </div>
      <div className="cmd" id="install-panel" role="tabpanel" aria-labelledby={`tab-${os}`}>
        <code>
          <span className="prompt" aria-hidden="true">
            {current.prompt}{' '}
          </span>
          {current.cmd}
        </code>
        <button type="button" className="copy" onClick={copy} aria-label={`Copy install command for ${current.tab}`}>
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>
      <p className="sr-only" role="status">
        {copied ? 'Install command copied to clipboard' : ''}
      </p>
      {/* PyPI package is not published yet; /install.sh and /install.ps1 redirect via public/_redirects. */}
      <p className="install-alt">
        Or <code>pip install qxide</code> (coming soon)
      </p>
    </section>
  );
}

// Real code from examples/bell.rs (comments shortened, printing omitted).
function BellCode() {
  return (
    <pre tabIndex={0} aria-label="Qxide example code: bell.rs">
      <code>
        <span className="tk-kw">use</span> qxide::<span className="tk-ty">Circuit</span>;{'\n\n'}
        <span className="tk-kw">fn</span> main() {'{'}
        {'\n'}
        {'    '}
        <span className="tk-kw">let mut</span> c = <span className="tk-ty">Circuit</span>::new();{'\n'}
        {'    '}
        <span className="tk-kw">let mut</span> a = c.qubit();{'\n'}
        {'    '}
        <span className="tk-kw">let mut</span> b = c.qubit();{'\n\n'}
        {'    '}c.h(&amp;<span className="tk-kw">mut</span> a);{'       '}
        <span className="tk-cm">// superposition</span>
        {'\n'}
        {'    '}c.cx(&amp;<span className="tk-kw">mut</span> a, &amp;<span className="tk-kw">mut</span> b);{' '}
        <span className="tk-cm">// entangle</span>
        {'\n\n'}
        {'    '}
        <span className="tk-kw">let</span> ma = c.measure(a);{' '}
        <span className="tk-cm">// a, b are consumed;</span>
        {'\n'}
        {'    '}
        <span className="tk-kw">let</span> mb = c.measure(b);{' '}
        <span className="tk-cm">// only classical bits remain</span>
        {'\n\n'}
        {'    '}
        <span className="tk-kw">let mut</span> counts = [<span className="tk-ty">0u32</span>; 4];{'\n'}
        {'    '}
        <span className="tk-kw">for</span> seed <span className="tk-kw">in</span> 0..1000 {'{'}
        {'\n'}
        {'        '}
        <span className="tk-kw">let</span> shot = c.simulate(seed);{'\n'}
        {'        '}
        <span className="tk-kw">let</span> k = (shot.get(ma) <span className="tk-kw">as</span> <span className="tk-ty">usize</span>) {'<<'} 1
        | shot.get(mb) <span className="tk-kw">as</span> <span className="tk-ty">usize</span>;{'\n'}
        {'        '}counts[k] += 1;{'\n'}
        {'    '}
        {'}'}
        {'\n'}
        {'}'}
      </code>
    </pre>
  );
}

// Output captured by running `cargo run -- bell` (same as `qxide bell`, 1000 simulated shots).
const QASM = `OPENQASM 3.0;
include "stdgates.inc";
qubit[2] q;
bit[2] c;
h q[0];
cx q[0], q[1];
c[0] = measure q[0];
c[1] = measure q[1];`;

const OUTCOMES = [
  {bits: '00', n: 503},
  {bits: '01', n: 0},
  {bits: '10', n: 0},
  {bits: '11', n: 497},
];

function Example() {
  return (
    <section className="example" aria-label="Example">
      <div className="example-grid">
        <div className="code-panel">
          <div className="panel-head">
            <span>examples/bell.rs</span>
          </div>
          <BellCode />
        </div>
        <div className="result-panel">
          <div className="panel-head">
            <h2>Output</h2>
            <span>Example output</span>
          </div>
          <div className="result-body">
            <p className="result-cmd">
              <span aria-hidden="true">$ </span>qxide bell
            </p>
            <pre className="result-qasm" tabIndex={0} aria-label="OpenQASM 3 output">
              {QASM}
            </pre>
            <ul className="bars" aria-label="Measurement counts over 1000 simulated shots">
              {OUTCOMES.map(({bits, n}) => (
                <li className="bar-row" key={bits}>
                  <span>{bits}</span>
                  <span className="bar-track" aria-hidden="true">
                    <span className="bar-fill" style={{display: 'block', width: `${n / 10}%`}} />
                  </span>
                  <span className="bar-n">{n}</span>
                </li>
              ))}
            </ul>
            <p className="result-note">
              Real output of <code>qxide bell</code>: the circuit as OpenQASM 3, then 1000 simulated shots. Because
              the two qubits are entangled, 01 and 10 never appear.
            </p>
          </div>
        </div>
      </div>
    </section>
  );
}

const FEATURES = [
  {
    n: '01',
    title: 'Two machines, one language',
    body: (
      <>
        Classical logic is ordinary Rust. The same circuit prints as OpenQASM 3 and runs on a built-in simulator:
        state-vector or stabilizer on the CPU, chosen automatically, plus a GPU prototype behind a feature flag.
      </>
    ),
  },
  {
    n: '02',
    title: 'Rust-style safety',
    body: (
      <>
        A <code>Qubit</code> can&apos;t be copied, and <code>measure</code> consumes it, so reuse after measurement
        won&apos;t compile. <code>with_ancilla</code> lends a helper qubit as control-only, then appends the inverse
        gates so it returns to |0⟩ and its slot can be reused.
      </>
    ),
  },
  {
    n: '03',
    title: 'Learn by running',
    body: (
      <>
        After installing, <code>qxide bell</code> prints the OpenQASM 3 for an entangled pair and the results of 1000
        simulated shots. The repository also ships runnable Rust examples such as <code>bell</code> and{' '}
        <code>ghz_large</code>.
      </>
    ),
  },
];

export function App() {
  return (
    <div className="paper">
      <header className="topbar">
        <div className="wrap">
          <a className="brand" href="/" aria-label="qxide.dev home">
            qxide<span className="dot">.</span>dev
          </a>
          <nav aria-label="Main">
            <ul className="nav">
              {NAV.map(({label, href, external}) => (
                <li key={label}>
                  <a href={href} {...(external ? {target: '_blank', rel: 'noreferrer'} : {})}>
                    {label}
                    {label === 'GitHub' ? ' ↗' : ''}
                  </a>
                </li>
              ))}
            </ul>
          </nav>
        </div>
      </header>

      <main className="wrap">
        <section className="hero">
          <p className="hero-kicker">fn main() → |ψ⟩</p>
          <h1>
            <span>Write it once.</span>
            <span>
              Run it on bits <span className="accent">and</span> qubits.
            </span>
          </h1>
          <p className="hero-sub">
            Today, Qxide is a Rust library and command-line tool for quantum circuits. A Rust-like .qx language for
            classical and quantum computers is planned.
          </p>
          <div className="cta">
            {/* TODO: point at the tutorial once the Learn page exists. */}
            <a className="pill pill-solid" href="#">
              Read the tutorial
            </a>
            <a className="pill pill-line" href={GITHUB} target="_blank" rel="noreferrer">
              GitHub
            </a>
          </div>
          <Install />
        </section>

        <Example />

        <section className="features" aria-label="Features">
          {FEATURES.map((f) => (
            <article className="feature" key={f.n}>
              <span className="feature-num">{f.n}</span>
              <h2>{f.title}</h2>
              <p>{f.body}</p>
            </article>
          ))}
        </section>
      </main>

      <footer className="footer">
        <div className="wrap">
          <span>Qxide · open source</span>
          <ul>
            <li>
              <a href={GITHUB} target="_blank" rel="noreferrer">
                GitHub
              </a>
            </li>
            <li>
              <a href="https://crates.io/crates/qxide" target="_blank" rel="noreferrer">
                crates.io
              </a>
            </li>
            <li>PyPI (coming soon)</li>
          </ul>
        </div>
      </footer>
    </div>
  );
}
