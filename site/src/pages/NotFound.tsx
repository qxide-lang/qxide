import {Link} from 'react-router';
import {usePageMeta} from '../lib/meta';

export function NotFound() {
  usePageMeta('Page not found | Qxide', 'This page does not exist on qxide.dev.');
  return (
    <section className="prose" aria-labelledby="nf-title">
      <header className="page-head">
        <h1 id="nf-title">Page not found</h1>
        <p className="lede">
          There is nothing at this address. Try the <Link to="/learn">tutorial</Link>, the{' '}
          <Link to="/docs">docs</Link> or the <Link to="/">home page</Link>.
        </p>
      </header>
    </section>
  );
}
