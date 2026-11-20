import {Link} from 'react-router';
import {CodeBlock} from '../components/CodeBlock';
import {usePageMeta} from '../lib/meta';

export function Playground() {
  usePageMeta(
    'Qxide playground: coming soon',
    'A browser playground for Qxide is planned. Until then, install the qxide command and try the examples.',
  );

  return (
    <article className="prose">
      <header className="page-head">
        <h1>Playground</h1>
        <p className="status" role="status">
          <span className="status-dot" aria-hidden="true" />
          In preparation. There is nothing to run in the browser yet.
        </p>
        <p className="lede">
          We want you to be able to write a Qxide program and run it in a browser tab, with nothing to install. That
          needs a few pieces that don&apos;t exist yet, so this page is a plan, not a product.
        </p>
      </header>

      <section aria-labelledby="plan">
        <h2 id="plan">The plan</h2>
        <ul className="plain-list">
          <li>
            <strong>Runs in your browser.</strong> The compiler and a state-vector simulator would be compiled to
            WebAssembly (WASM), so your code never leaves your machine.
          </li>
          <li>
            <strong>Qubit limits.</strong> The goal is 20 qubits by default and up to 24 on desktop browsers. A state
            vector doubles in size with every qubit, so a browser tab can&apos;t go much further.
          </li>
          <li>
            <strong>Released with the .qx language.</strong> The playground is meant for the planned <code>.qx</code>{' '}
            language, so it will launch when that language does. See{' '}
            <Link to="/blog/why-qxide">Why Qxide</Link> for the roadmap.
          </li>
        </ul>
        <p>These are goals, not commitments to a date.</p>
      </section>

      <section aria-labelledby="now">
        <h2 id="now">What you can do today</h2>
        <p>Install the command and run the Bell-state circuit on your own machine:</p>
        <CodeBlock
          label="Install script (macOS / Linux)"
          lang="sh"
          code="curl --proto '=https' --tlsv1.2 -LsSf https://qxide.dev/install.sh | sh"
        />
        <CodeBlock
          label="Install script (Windows)"
          lang="sh"
          code='powershell -ExecutionPolicy Bypass -c "irm https://qxide.dev/install.ps1 | iex"'
        />
        <CodeBlock label="Run it" lang="sh" code="qxide bell" />
        <p>
          Then follow the <Link to="/learn">tutorial</Link> to write your own circuits in Rust, or run the programs on
          the <Link to="/examples">examples page</Link>.
        </p>
      </section>
    </article>
  );
}
