import {useEffect, useRef} from 'react';
import {Link, NavLink, Outlet, useLocation} from 'react-router';
import {GITHUB} from '../lib/meta';
import {useMotion} from '../lib/motion';

const NAV = [
  {label: 'Learn', to: '/learn'},
  {label: 'Docs', to: '/docs'},
  {label: 'Playground', to: '/playground'},
  {label: 'Examples', to: '/examples'},
  {label: 'Blog', to: '/blog'},
];

export function SiteHeader() {
  return (
    <header className="topbar">
      <div className="wrap">
        <Link className="brand" to="/" aria-label="qxide.dev home">
          <img className="brand-mark" src="/qxide-mark.svg" alt="" width="26" height="26" />
          <span>
            qxide<span className="dot">.</span>dev
          </span>
        </Link>
        <nav aria-label="Main">
          <ul className="nav">
            {NAV.map(({label, to}) => (
              <li key={to}>
                <NavLink to={to}>{label}</NavLink>
              </li>
            ))}
            <li>
              <a href={GITHUB} target="_blank" rel="noreferrer">
                GitHub<span aria-hidden="true"> ↗</span>
                <span className="sr-only"> (opens in a new tab)</span>
              </a>
            </li>
          </ul>
        </nav>
      </div>
    </header>
  );
}

export function SiteFooter() {
  return (
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
          <li>
            <a href="https://pypi.org/project/qxide/" target="_blank" rel="noreferrer">
              PyPI
            </a>
          </li>
          <li>
            <a href="https://docs.rs/qxide" target="_blank" rel="noreferrer">
              docs.rs
            </a>
          </li>
        </ul>
      </div>
    </footer>
  );
}

/** Shared frame for every page: skip link, top bar, routed content, footer. */
export function Layout() {
  const {pathname, hash} = useLocation();
  const paperRef = useRef<HTMLDivElement>(null);
  useMotion(paperRef, pathname);
  useEffect(() => {
    const target = hash ? document.getElementById(decodeURIComponent(hash.slice(1))) : null;
    if (target) target.scrollIntoView();
    else window.scrollTo(0, 0);
  }, [pathname, hash]);

  return (
    <div className="paper" ref={paperRef}>
      <a className="skip" href="#main">
        Skip to content
      </a>
      <SiteHeader />
      <main id="main" key={pathname} className="wrap page-fade">
        <Outlet />
      </main>
      <SiteFooter />
    </div>
  );
}
