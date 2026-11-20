import {Fragment, useState, type ReactNode} from 'react';

const KEYWORDS = new Set(['use', 'fn', 'let', 'mut', 'for', 'in', 'as', 'if', 'else', 'move', 'pub', 'struct', 'impl']);

// Minimal Rust highlighter: comments, strings, keywords and capitalised types. Enough for the short snippets here.
function highlightRust(code: string): ReactNode[] {
  const re = /(\/\/[^\n]*)|("(?:[^"\\]|\\.)*")|\b([A-Za-z_][A-Za-z0-9_]*)\b/g;
  const out: ReactNode[] = [];
  let last = 0;
  let key = 0;
  for (const m of code.matchAll(re)) {
    const i = m.index ?? 0;
    if (i > last) out.push(code.slice(last, i));
    if (m[1]) out.push(<span key={key++} className="tk-cm">{m[1]}</span>);
    else if (m[2]) out.push(<span key={key++} className="tk-str">{m[2]}</span>);
    else if (KEYWORDS.has(m[3])) out.push(<span key={key++} className="tk-kw">{m[3]}</span>);
    else if (/^[A-Z]/.test(m[3])) out.push(<span key={key++} className="tk-ty">{m[3]}</span>);
    else out.push(m[3]);
    last = i + m[0].length;
  }
  if (last < code.length) out.push(code.slice(last));
  return out;
}

type Props = {
  code: string;
  /** Shown in the header (file name or short label); also used in the copy button's accessible name. */
  label: string;
  lang?: 'rust' | 'sh' | 'text' | 'toml';
  /** Output blocks have no copy button. */
  copyable?: boolean;
};

export function CodeBlock({code, label, lang = 'text', copyable = true}: Props) {
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      setCopied(false);
    }
  };

  return (
    <figure className="codeblock">
      <figcaption className="codeblock-head">
        <span>{label}</span>
        {copyable && (
          <button type="button" className="copy" onClick={copy} aria-label={`Copy ${label}`}>
            {copied ? 'Copied' : 'Copy'}
          </button>
        )}
      </figcaption>
      <pre tabIndex={0} aria-label={label}>
        <code>{lang === 'rust' ? <Fragment>{highlightRust(code)}</Fragment> : code}</code>
      </pre>
      {copyable && (
        <p className="sr-only" role="status">
          {copied ? `${label} copied to clipboard` : ''}
        </p>
      )}
    </figure>
  );
}
