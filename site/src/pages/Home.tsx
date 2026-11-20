import {useEffect, useState, type CSSProperties} from 'react';
import {Link} from 'react-router';
import {highlightQx} from '../components/CodeBlock';
import {CopyLabel} from '../components/CopyLabel';
import {WAVES, WAVE_WIDTH} from '../lib/waves';
import {GITHUB, usePageMeta} from '../lib/meta';

type Os = 'unix' | 'windows';

// /install.sh and /install.ps1 redirect to the GitHub release installers via public/_redirects (see docs/release.md).
const INSTALL: Record<Os, {tab: string; prompt: string; cmd: string}> = {
  unix: {
    tab: 'macOS / Linux',
    prompt: '$',
    cmd: "curl --proto '=https' --tlsv1.2 -LsSf https://qxide.dev/install.sh | sh",
  },
  windows: {
    tab: 'Windows',
    prompt: 'PS>',
    cmd: 'powershell -ExecutionPolicy Bypass -c "irm https://qxide.dev/install.ps1 | iex"',
  },
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
          <CopyLabel copied={copied} />
        </button>
      </div>
      <p className="sr-only" role="status">
        {copied ? 'Install command copied to clipboard' : ''}
      </p>
      {/* /install.sh and /install.ps1 redirect via public/_redirects. */}
      <p className="install-alt">
        Or <code>cargo install qxide</code> · <code>pip install qxide</code>
      </p>
    </section>
  );
}

// Real code from examples/qx/bell.qx.
const BELL_QX = `// Bell pair: the two measured bits are always equal.
void main() {
    Qubit a = qubit();
    Qubit b = qubit();
    h(a);
    cx(a, b);
    Bit ma = measure(a);
    Bit mb = measure(b);
    println("a={} b={}", ma, mb);
}`;

function BellCode() {
  return (
    <pre tabIndex={0} aria-label="Qxide example code: bell.qx">
      <code>{highlightQx(BELL_QX)}</code>
    </pre>
  );
}

// Output captured by running `qxide run examples/qx/bell.qx --shots 1000` and `qxide qasm examples/qx/bell.qx`.
const RUN_OUT = `[0, 0]  0b00 = 0  ×504
  a=0 b=0
[1, 1]  0b11 = 3  ×496
  a=1 b=1`;

const QASM = `OPENQASM 3.0;
include "stdgates.inc";
qubit[2] q;
bit[2] c;
h q[0];
cx q[0], q[1];
c[0] = measure q[0];
c[1] = measure q[1];`;

const OUTCOMES = [
  {bits: '[0, 0]', n: 504},
  {bits: '[1, 1]', n: 496},
];

function Example() {
  return (
    <section className="example" aria-label="Example">
      <div className="example-grid">
        <div className="code-panel">
          <div className="panel-head">
            <span>examples/qx/bell.qx</span>
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
              <span aria-hidden="true">$ </span>qxide run bell.qx --shots 1000
            </p>
            <pre className="result-qasm" tabIndex={0} aria-label="Output of qxide run with 1000 shots">
              {RUN_OUT}
            </pre>
            <ul className="bars" aria-label="Measurement counts over 1000 simulated shots">
              {OUTCOMES.map(({bits, n}) => (
                <li className="bar-row" key={bits}>
                  <span>{bits}</span>
                  <span className="bar-track" aria-hidden="true">
                    <span className="bar-fill" style={{'--p': n / 1000} as CSSProperties} />
                  </span>
                  <span className="bar-n">{n}</span>
                </li>
              ))}
            </ul>
            <p className="result-cmd">
              <span aria-hidden="true">$ </span>qxide qasm bell.qx
            </p>
            <pre className="result-qasm" tabIndex={0} aria-label="OpenQASM 3 output">
              {QASM}
            </pre>
            <p className="result-note">
              Real output of <code>qxide run</code> and <code>qxide qasm</code>. Because the two qubits are
              entangled, only <code>[0, 0]</code> and <code>[1, 1]</code> ever appear.
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
        Classical logic and quantum operations live in one <code>.qx</code> program with familiar C-style syntax.{' '}
        <code>qxide run</code> simulates it with built-in state-vector and stabilizer simulators, and{' '}
        <code>qxide qasm</code> exports OpenQASM 3.
      </>
    ),
  },
  {
    n: '02',
    title: 'Compile-time qubit safety',
    body: (
      <>
        Qubit safety is checked at compile time, like Rust&apos;s ownership. <code>qxide check</code> rejects use
        after measurement, the same qubit passed twice to one gate, and qubits that are never measured or discarded,
        before anything runs.
      </>
    ),
  },
  {
    n: '03',
    title: 'Learn by running',
    body: (
      <>
        After installing, run <code>qxide run examples/qx/bell.qx --shots 1000</code> to see the outcomes as{' '}
        <code>[0, 1, 1]</code>, <code>0b110</code> and an integer. The same engine is also a Rust library, with a GPU
        prototype behind a feature flag.
      </>
    ),
  },
];

export function Home() {
  usePageMeta(
    'Qxide: a programming language for classical and quantum computers',
    "Qxide is a programming language for classical and quantum computers: familiar C-style syntax, compile-time qubit safety like Rust's ownership, and OpenQASM 3 output.",
  );
  return (
    <>
      <section className="hero">
        <div className="hero-bg" aria-hidden="true">
          <svg className="waves" viewBox={`0 0 ${WAVE_WIDTH} 300`} width={WAVE_WIDTH} height="300" focusable="false">
            {WAVES.map((w) => (
              <path key={w.cls} className={w.cls} d={w.d} />
            ))}
          </svg>
          <span className="qdot qdot-1" />
          <span className="qdot qdot-2" />
          <span className="qdot qdot-3" />
        </div>
        <p className="hero-kicker">void main() → |ψ⟩</p>
        <h1>
          <span>Write it once.</span>
          <span>
            Run it on bits <span className="accent">and</span> qubits.
          </span>
        </h1>
        <p className="hero-sub">
          Familiar C-style syntax, compile-time qubit safety like Rust&apos;s ownership, and OpenQASM 3 output.
          Version 0.5.0 is an early experiment; the language may still change.
        </p>
        <div className="cta">
          <Link className="pill pill-solid" to="/learn">
            Read the tutorial
          </Link>
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
    </>
  );
}
