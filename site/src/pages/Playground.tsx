import {useEffect, useId, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent} from 'react';
import {Link} from 'react-router';
import {CopyLabel} from '../components/CopyLabel';
import {usePageMeta} from '../lib/meta';
import {EngineClient, TIME_LIMIT_MS, type RunResult} from '../playground/client';
import {Editor} from '../playground/Editor';
import type {Command} from '../playground/engine';
import {EXAMPLES} from '../playground/examples';
import {offsetOf, parseCounts, parseDiagnostics, type CountRow} from '../playground/output';
import {decodeShare, encodeShare} from '../playground/share';
import '../playground/playground.css';

// Mirrors crates/qxide-wasm/src/lib.rs (MAX_SHOTS, MAX_STATE_VECTOR_QUBITS).
const MAX_SHOTS = 100_000;
const MAX_QUBITS = 20;
const U64_MAX = (1n << 64n) - 1n;
const SHARED_NAME = 'playground.qx';
const MAX_BARS = 32;

type Tab = 'output' | 'counts' | 'qasm';
// `seed` is the seed a run used (null for check and qasm); `randomSeed` when the field was empty.
type Finished = {cmd: Command; echo: string; result: RunResult; name: string; seed: string | null; randomSeed: boolean};

const fill = (template: string, values: Record<string, string | number>) =>
  template.replace(/\{(\w+)\}/g, (m, k: string) => (k in values ? String(values[k]) : m));

function validShots(v: string): boolean {
  return /^\d{1,6}$/.test(v) && Number(v) >= 1 && Number(v) <= MAX_SHOTS;
}

// An empty seed is valid: each run then draws its own.
function validSeed(v: string): boolean {
  return v === '' || (/^\d{1,20}$/.test(v) && BigInt(v) <= U64_MAX);
}

function randomSeed(): string {
  return crypto.getRandomValues(new BigUint64Array(1))[0].toString();
}

function formatMs(ms: number): string {
  return ms < 1000 ? `${Math.max(1, Math.round(ms))} ms` : `${(ms / 1000).toFixed(2)} s`;
}

function statusText(f: Finished | null, running: Command | null): string {
  if (running) {
    const RUNNING: Record<Command, string> = {
      run: "Simulating…",
      check: "Checking…",
      qasm: "Exporting OpenQASM 3…",
    };
    return RUNNING[running];
  }
  if (!f) return '';
  const r = f.result;
  switch (r.kind) {
    case 'done':
      return r.outcome.exit === 0
        ? fill("Finished in {time}", {time: formatMs(r.ms)})
        : fill("Exited with code {code} in {time}", {code: r.outcome.exit, time: formatMs(r.ms)});
    case 'stopped':
      return "Stopped";
    case 'timeout':
      return fill("Timed out after {seconds} seconds", {seconds: TIME_LIMIT_MS / 1000});
    default:
      return "Playground engine error";
  }
}

function failureText(r: RunResult): string | null {
  switch (r.kind) {
    case 'trap':
      if (r.reason === 'stack')
        return "The browser call stack overflowed inside the WebAssembly engine. Reduce recursion depth or block nesting, or run the program with the installed qxide run command, which has a larger stack.";
      if (r.reason === 'memory')
        return "The browser ran out of memory while simulating the program. Use fewer qubits or shots, or run the program with the installed qxide run command.";
      return "The browser engine crashed with an internal error unrelated to your program. Run it with the installed qxide run command and report the issue on GitHub.";
    case 'load-error':
      return "Could not load the playground WebAssembly engine. Reload the page to try again; browsers without WebAssembly support cannot run the playground.";
    case 'timeout':
      return fill("Run stopped after {seconds} seconds. Every shot simulates the whole circuit, so lower the shot count or qubit count, or use the installed qxide run command, which has no time limit.", {seconds: TIME_LIMIT_MS / 1000});
    case 'stopped':
      return "Run cancelled before producing a result.";
    default:
      return null;
  }
}

function Counts({rows}: {rows: CountRow[]}) {
  // Bars start empty and grow once mounted (see the .motion rules in pages.css).
  const [grown, setGrown] = useState(false);
  useEffect(() => {
    const raf = requestAnimationFrame(() => setGrown(true));
    return () => cancelAnimationFrame(raf);
  }, []);
  const total = rows.reduce((n, r) => n + r.count, 0);
  const max = Math.max(...rows.map((r) => r.count));
  const shown = rows.slice(0, MAX_BARS);
  return (
    <>
      <ol className={grown ? 'bars pg-bars is-visible' : 'bars pg-bars'}>
        {shown.map((r) => {
          const pct = ((100 * r.count) / total).toFixed(1);
          return (
            <li key={r.bits} className="pg-bar">
              <span className="bar-row pg-bar-row">
                <code className="pg-bar-label" title={r.bits}>
                  {r.binary}
                </code>
                <span className="bar-track" aria-hidden="true">
                  <span className="bar-fill" style={{'--p': r.count / max} as CSSProperties} />
                </span>
                <span className="bar-n">
                  {r.count}
                  <span className="pg-pct"> ({pct}%)</span>
                </span>
              </span>
              <span className="pg-bar-meta">
                <code>{r.bits}</code>
                {r.value !== null && <code> = {r.value}</code>}
                {r.lines.map((l, i) => (
                  <code key={i} className="pg-bar-out">
                    {l}
                  </code>
                ))}
              </span>
            </li>
          );
        })}
      </ol>
      {rows.length > shown.length && (
        <p className="result-note">
          {fill("Showing the top {shown} outcomes; {hidden} less frequent outcomes are listed in the Output tab.", {shown: shown.length, hidden: rows.length - shown.length})}
        </p>
      )}
    </>
  );
}

export function Playground() {
  usePageMeta(
    "Qxide playground",
    "Write .qx programs in the browser: check, run with shots and a seed, or export to OpenQASM 3. Runs locally in WebAssembly with the same output as qxide.",
  );

  const ids = useId();
  const editorId = `${ids}-editor`;
  const hintId = `${ids}-hint`;
  const shotsErrId = `${ids}-shots-err`;
  const seedErrId = `${ids}-seed-err`;

  const [code, setCode] = useState(EXAMPLES[0]?.code ?? '');
  const [name, setName] = useState(EXAMPLES[0]?.file ?? SHARED_NAME);
  const [shots, setShots] = useState('1000');
  const [seed, setSeed] = useState('');
  const [running, setRunning] = useState<Command | null>(null);
  const [finished, setFinished] = useState<Finished | null>(null);
  const [qasm, setQasm] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>('output');
  const [touched, setTouched] = useState({shots: false, seed: false});
  const [notice, setNotice] = useState('');
  const [copied, setCopied] = useState<'link' | 'qasm' | null>(null);
  // Bumped per finished command so the bar chart remounts and grows again.
  const [runKey, setRunKey] = useState(0);

  const client = useRef<EngineClient | null>(null);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const shotsInput = useRef<HTMLInputElement>(null);
  const seedInput = useRef<HTMLInputElement>(null);
  const tabRefs = useRef<Record<Tab, HTMLButtonElement | null>>({output: null, counts: null, qasm: null});

  useEffect(() => {
    client.current = new EngineClient();
    return () => client.current?.dispose();
  }, []);

  // Open a shared program from the URL hash on load and when a link is pasted into this tab
  // (a hash-only navigation does not reload the page; our own replaceState fires no hashchange).
  useEffect(() => {
    let live = true;
    const open = () => {
      decodeShare(window.location.hash)
        .then((shared) => {
          if (!live || !shared) return;
          setCode(shared.code);
          setName(SHARED_NAME);
          setFinished(null);
          setQasm(null);
          setNotice('');
          if (shared.shots && validShots(shared.shots)) setShots(shared.shots);
          // A link without a seed means a random seed per run, not the seed typed before.
          setSeed(shared.seed && validSeed(shared.seed) ? shared.seed : '');
        })
        .catch(() => {
          if (!live) return;
          setCode(EXAMPLES[0]?.code ?? '');
          setName(EXAMPLES[0]?.file ?? SHARED_NAME);
          setNotice("The shared link is invalid or too large to load, so the default example is shown instead.");
        });
    };
    open();
    window.addEventListener('hashchange', open);
    return () => {
      live = false;
      window.removeEventListener('hashchange', open);
    };
  }, []);

  const shotsOk = validShots(shots);
  const seedOk = validSeed(seed);

  const outcome = finished?.result.kind === 'done' ? finished.result.outcome : null;
  const diagnostics = useMemo(
    () => (outcome && finished ? parseDiagnostics(outcome.stderr, finished.name) : []),
    [outcome, finished],
  );
  const errorLines = useMemo(() => new Set(diagnostics.map((d) => d.line)), [diagnostics]);
  const counts = useMemo(
    () => (outcome && finished?.cmd === 'run' && outcome.exit === 0 ? parseCounts(outcome.stdout) : null),
    [outcome, finished],
  );

  const exec = async (cmd: Command) => {
    const c = client.current;
    if (!c) return;
    if (cmd === 'run' && (!shotsOk || !seedOk)) {
      setTouched({shots: true, seed: true});
      (shotsOk ? seedInput : shotsInput).current?.focus();
      return;
    }
    // Resolved before the echo and the request, so both name the seed the run really used.
    const randomRun = cmd === 'run' && seed === '';
    const used = cmd === 'run' ? (randomRun ? randomSeed() : seed) : null;
    const echo = cmd === 'run' ? `qxide run ${name} --shots ${shots} --seed ${used}` : `qxide ${cmd} ${name}`;
    setRunning(cmd);
    const result = await c.run({cmd, name, src: code, shots: cmd === 'run' ? shots : '1', seed: used ?? '0'});
    setRunning(null);
    setFinished({cmd, echo, result, name, seed: used, randomSeed: randomRun});
    setRunKey((k) => k + 1);
    const ok = result.kind === 'done' && result.outcome.exit === 0;
    if (cmd === 'qasm' && ok) {
      setQasm(result.outcome.stdout);
      setTab('qasm');
    } else if (cmd === 'run' && ok && parseCounts(result.outcome.stdout)) {
      setTab('counts');
    } else {
      setTab('output');
    }
  };

  const jumpTo = (line: number, col: number) => {
    const ta = textarea.current;
    if (!ta) return;
    const at = offsetOf(code, line, col);
    ta.focus();
    ta.setSelectionRange(at, at);
  };

  const chooseExample = (file: string) => {
    const ex = EXAMPLES.find((e) => e.file === file);
    if (!ex) return;
    setCode(ex.code);
    setName(ex.file);
    setFinished(null);
    setQasm(null);
    setNotice('');
  };

  const flash = (what: 'link' | 'qasm') => {
    setCopied(what);
    window.setTimeout(() => setCopied((c) => (c === what ? null : c)), 1800);
  };

  const share = async () => {
    // With an empty field, share the seed of the last run so the link reproduces it.
    const hash = await encodeShare({code, shots, seed: seed || finished?.seed || null});
    const url = `${window.location.origin}${window.location.pathname}${hash}`;
    // replaceState keeps React Router's location (and its scroll reset) out of it.
    window.history.replaceState(window.history.state, '', url);
    try {
      await navigator.clipboard.writeText(url);
      setNotice('');
      flash('link');
    } catch {
      setNotice("Clipboard access was denied; the share link is now in the address bar.");
    }
  };

  const copyQasm = async () => {
    if (!qasm) return;
    try {
      await navigator.clipboard.writeText(qasm);
      flash('qasm');
    } catch {
      setCopied(null);
    }
  };

  const TABS: {key: Tab; label: string}[] = [
    {key: 'output', label: "Output"},
    {key: 'counts', label: "Counts"},
    {key: 'qasm', label: 'QASM'},
  ];

  const onTabKey = (e: KeyboardEvent<HTMLButtonElement>, i: number) => {
    const next = e.key === 'ArrowRight' ? i + 1 : e.key === 'ArrowLeft' ? i - 1 : e.key === 'Home' ? 0 : e.key === 'End' ? TABS.length - 1 : null;
    if (next === null) return;
    e.preventDefault();
    const t = TABS[(next + TABS.length) % TABS.length].key;
    setTab(t);
    tabRefs.current[t]?.focus();
  };

  const failure = finished ? failureText(finished.result) : null;
  const status = statusText(finished, running);

  return (
    <article className="pg">
      <header className="page-head pg-head">
        <h1>Playground</h1>
        <p className="lede">
          {"Write a .qx program here to check it, run it, or export it to OpenQASM 3. Everything runs locally in your browser in WebAssembly with nothing sent to a server, producing the same output as the qxide command."}
        </p>
      </header>

      {notice && (
        <p className="pg-notice" role="alert">
          {notice}
        </p>
      )}

      <form
        className="pg-toolbar"
        aria-label={"Playground controls"}
        onSubmit={(e) => {
          e.preventDefault();
          void exec('run');
        }}
        noValidate
      >
        <label className="pg-field">
          <span className="pg-field-label">{"Example"}</span>
          <select
            className="pg-select"
            value={EXAMPLES.some((e) => e.file === name) ? name : ''}
            onChange={(e) => chooseExample(e.target.value)}
          >
            {!EXAMPLES.some((e) => e.file === name) && <option value="">{name}</option>}
            {EXAMPLES.map((e) => (
              <option key={e.file} value={e.file}>
                {e.file}
              </option>
            ))}
          </select>
        </label>
        <label className="pg-field">
          <span className="pg-field-label">{"Shots"}</span>
          <input
            ref={shotsInput}
            className="pg-num"
            inputMode="numeric"
            value={shots}
            onChange={(e) => setShots(e.target.value.trim())}
            onBlur={() => setTouched((t) => ({...t, shots: true}))}
            aria-invalid={!shotsOk || undefined}
            aria-describedby={!shotsOk && touched.shots ? shotsErrId : undefined}
          />
        </label>
        <label className="pg-field">
          <span className="pg-field-label">{"Seed"}</span>
          <input
            ref={seedInput}
            className="pg-num pg-seed"
            inputMode="numeric"
            value={seed}
            placeholder={"random"}
            onChange={(e) => setSeed(e.target.value.trim())}
            onBlur={() => setTouched((t) => ({...t, seed: true}))}
            aria-invalid={!seedOk || undefined}
            aria-describedby={!seedOk && touched.seed ? seedErrId : undefined}
          />
        </label>
        <span className="pg-actions">
          {running ? (
            <button type="button" className="pill pill-solid pg-btn" onClick={() => client.current?.stop()}>
              {"Stop"}
            </button>
          ) : (
            <button type="submit" className="pill pill-solid pg-btn" aria-keyshortcuts="Control+Enter">
              {"Run"}
            </button>
          )}
          <button type="button" className="pill pill-line pg-btn" disabled={running !== null} onClick={() => void exec('check')}>
            {"Check"}
          </button>
          <button type="button" className="pill pill-line pg-btn" disabled={running !== null} onClick={() => void exec('qasm')}>
            QASM
          </button>
          <button type="button" className="pill pill-line pg-btn" onClick={() => void share()}>
            {copied === 'link'
              ? "Link copied"
              : "Copy link"}
          </button>
        </span>
        {!shotsOk && touched.shots && (
          <p id={shotsErrId} className="pg-field-err">
            {fill("Enter a whole number from 1 to {max}.", {max: MAX_SHOTS.toLocaleString('en-US')})}
          </p>
        )}
        {!seedOk && touched.seed && (
          <p id={seedErrId} className="pg-field-err">
            {fill("Enter a whole number from 0 to {max}.", {max: U64_MAX.toString()})}
          </p>
        )}
      </form>

      <section className="pg-grid">
        <section className="code-panel pg-code-panel" aria-labelledby={`${ids}-editor-label`}>
          <header className="panel-head">
            <label id={`${ids}-editor-label`} htmlFor={editorId}>
              {name}
            </label>
            <span className="pg-lines">{fill("{n} lines", {n: code.split('\n').length})}</span>
          </header>
          <Editor
            id={editorId}
            value={code}
            onChange={setCode}
            errorLines={errorLines}
            onRun={() => void exec('run')}
            describedBy={hintId}
            textareaRef={textarea}
          />
          <p id={hintId} className="pg-hint">
            {"Press Ctrl+Enter or Cmd+Enter to run. Use Tab to indent, Shift+Tab to outdent, and Esc then Tab to leave the editor."}
          </p>
        </section>

        <section className="result-panel pg-result" aria-labelledby={`${ids}-result-title`} aria-busy={running !== null}>
          <header className="panel-head">
            <h2 id={`${ids}-result-title`}>{"Results"}</h2>
            <span className="pg-status" role="status">
              {status}
            </span>
          </header>
          <section className="result-body">
            {finished && (
              <p className="result-cmd pg-echo">
                <code>$ {finished.echo}</code>
              </p>
            )}
            {finished?.randomSeed && finished.seed && outcome?.exit === 0 && (
              <p className="pg-seed-note">
                {fill("Ran with random seed {seed}; enter it in the Seed field or use Copy link to reproduce this run.", {seed: finished.seed})}
              </p>
            )}
            <section className="tabs pg-tabs" role="tablist" aria-label={"Result views"}>
              {TABS.map((t, i) => (
                <button
                  key={t.key}
                  ref={(el) => {
                    tabRefs.current[t.key] = el;
                  }}
                  type="button"
                  role="tab"
                  id={`${ids}-tab-${t.key}`}
                  aria-controls={`${ids}-panel-${t.key}`}
                  aria-selected={tab === t.key}
                  tabIndex={tab === t.key ? 0 : -1}
                  className="tab pg-tab"
                  onClick={() => setTab(t.key)}
                  onKeyDown={(e) => onTabKey(e, i)}
                >
                  {t.label}
                </button>
              ))}
            </section>

            <section
              role="tabpanel"
              id={`${ids}-panel-output`}
              aria-labelledby={`${ids}-tab-output`}
              hidden={tab !== 'output'}
              className="pg-panel"
            >
              {!finished && (
                <p className="pg-empty">{"Press Run, Check, or QASM, or use Ctrl+Enter in the editor to start."}</p>
              )}
              {failure && <p className="pg-failure">{failure}</p>}
              {diagnostics.length > 0 && (
                <ul className="pg-diags">
                  {diagnostics.map((d, i) => (
                    <li key={i}>
                      <button type="button" className="pg-diag" onClick={() => jumpTo(d.line, d.col)}>
                        <code className="pg-diag-code">{d.code}</code>
                        <span className="pg-diag-loc">
                          {fill("Line {line}, col {col}", {line: d.line, col: d.col})}
                        </span>
                        <span className="pg-diag-msg">{d.message}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              {outcome && outcome.stderr && (
                <pre className="pg-out pg-stderr" tabIndex={0} aria-label="stderr">
                  {outcome.stderr}
                </pre>
              )}
              {outcome && outcome.stdout && (
                <pre className="pg-out" tabIndex={0} aria-label="stdout">
                  {outcome.stdout}
                </pre>
              )}
              {outcome && !outcome.stdout && !outcome.stderr && (
                <p className="pg-empty">{"Program finished with no output."}</p>
              )}
            </section>

            <section
              role="tabpanel"
              id={`${ids}-panel-counts`}
              aria-labelledby={`${ids}-tab-counts`}
              hidden={tab !== 'counts'}
              className="pg-panel"
            >
              {counts ? (
                <Counts key={runKey} rows={counts} />
              ) : (
                <p className="pg-empty">{"Run a program that measures qubits with multiple shots to see how often each outcome occurs."}</p>
              )}
            </section>

            <section
              role="tabpanel"
              id={`${ids}-panel-qasm`}
              aria-labelledby={`${ids}-tab-qasm`}
              hidden={tab !== 'qasm'}
              className="pg-panel"
            >
              {qasm ? (
                <figure className="pg-qasm">
                  <figcaption className="pg-qasm-head">
                    <span>OpenQASM 3</span>
                    <button type="button" className="copy pg-copy" onClick={() => void copyQasm()} aria-label="Copy OpenQASM 3">
                      <CopyLabel copied={copied === 'qasm'} />
                    </button>
                  </figcaption>
                  <pre className="result-qasm pg-out" tabIndex={0} aria-label="OpenQASM 3">
                    {qasm}
                  </pre>
                </figure>
              ) : (
                <p className="pg-empty">{"Press QASM to export the program's circuit as OpenQASM 3."}</p>
              )}
            </section>
          </section>
        </section>
      </section>

      <section className="prose pg-notes" aria-labelledby={`${ids}-limits`}>
        <h2 id={`${ids}-limits`}>{"Runtime and limits"}</h2>
        <ul className="plain-list">
          <li>
            {"The playground runs the qxide interpreter in WebAssembly inside a Web Worker, so your code is never uploaded. Check, run, and QASM match the qxide command, including error codes with line and column, and the same seed gives the same counts."}
          </li>
          <li>
            {fill("Non-Clifford gates (t, rx, ry, rz, ccx, and others) use the state-vector simulator, capped at {qubits} qubits (2^{qubits} amplitudes, 16 MiB); Clifford-only circuits use the stabilizer simulator for many more qubits. Programs over the cap stop with error Q0406.", {qubits: MAX_QUBITS})}
          </li>
          <li>
            {fill("Runs are limited to {shots} shots and stop after {seconds} seconds, and you can press Stop at any time. Every shot simulates the full circuit again, so large circuits with many shots take longer.", {shots: MAX_SHOTS.toLocaleString('en-US'), seconds: TIME_LIMIT_MS / 1000})}
          </li>
        </ul>
        <p>
          {"To run the same programs on your machine without limits using the qxide command, see "}
          <Link to="/learn">{"the installation tutorial"}</Link>.
        </p>
      </section>
    </article>
  );
}
