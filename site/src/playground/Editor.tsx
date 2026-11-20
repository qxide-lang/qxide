import {useRef, type KeyboardEvent, type RefObject} from 'react';
import {highlightQx} from '../components/CodeBlock';

type Props = {
  id: string;
  value: string;
  onChange: (value: string) => void;
  /** 1-based lines that carry a diagnostic. */
  errorLines: ReadonlySet<number>;
  onRun: () => void;
  describedBy: string;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
};

const INDENT = '    ';

// Inserts through the browser's editing commands so undo/redo keeps working; falls back to a plain value update.
function insert(ta: HTMLTextAreaElement, text: string, onChange: (v: string) => void) {
  const done = text ? document.execCommand?.('insertText', false, text) : document.execCommand?.('delete');
  if (done) return;
  const {selectionStart: s, selectionEnd: e, value} = ta;
  const next = value.slice(0, s) + text + value.slice(e);
  onChange(next);
  requestAnimationFrame(() => ta.setSelectionRange(s + text.length, s + text.length));
}

/**
 * A textarea over a highlighted copy of the same text. Both sit in one scroll box and size to the
 * content, so line numbers, highlighting and the caret stay aligned without scroll syncing.
 */
export function Editor({id, value, onChange, errorLines, onRun, describedBy, textareaRef}: Props) {
  // After Escape, Tab moves focus out of the editor instead of indenting.
  const tabReleased = useRef(false);
  const box = useRef<HTMLElement>(null);
  const charWidth = useRef(0);
  const lines = value.split('\n');

  // The textarea never scrolls itself (it is as large as its text), so keep the caret inside the scroll box.
  const revealCaret = () => {
    const ta = textareaRef.current;
    const el = box.current;
    if (!ta || !el) return;
    const style = getComputedStyle(ta);
    const lh = parseFloat(style.lineHeight) || 24;
    if (!charWidth.current) {
      const ctx = document.createElement('canvas').getContext('2d');
      if (ctx) ctx.font = `${style.fontSize} ${style.fontFamily}`;
      charWidth.current = ctx?.measureText('0').width || 8;
    }
    const before = ta.value.slice(0, ta.selectionEnd);
    const row = before.split('\n').length - 1;
    const col = before.length - before.lastIndexOf('\n') - 1;
    const top = parseFloat(style.paddingTop) + row * lh;
    if (top < el.scrollTop) el.scrollTop = top - lh;
    else if (top + 2 * lh > el.scrollTop + el.clientHeight) el.scrollTop = top + 2 * lh - el.clientHeight;
    // The sticky gutter covers the left edge of the scroll box.
    const gutter = (ta.parentElement?.offsetLeft ?? 0);
    const left = gutter + parseFloat(style.paddingLeft) + col * charWidth.current;
    const margin = 4 * charWidth.current;
    if (left - margin < el.scrollLeft + gutter) el.scrollLeft = Math.max(0, left - margin - gutter);
    else if (left + margin > el.scrollLeft + el.clientWidth) el.scrollLeft = left + margin - el.clientWidth;
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    const ta = e.currentTarget;
    // Leave Enter/Tab to the input method while Korean (or other IME) text is being composed.
    if (e.nativeEvent.isComposing || e.keyCode === 229) return;
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault();
      onRun();
      return;
    }
    if (e.key === 'Escape') {
      tabReleased.current = true;
      return;
    }
    if (e.key === 'Tab' && !tabReleased.current && !e.altKey && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      if (e.shiftKey) {
        const start = ta.value.lastIndexOf('\n', ta.selectionStart - 1) + 1;
        const lead = /^ {1,4}/.exec(ta.value.slice(start))?.[0].length ?? 0;
        if (lead) {
          ta.setSelectionRange(start, start + lead);
          insert(ta, '', onChange);
        }
      } else {
        insert(ta, INDENT, onChange);
      }
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey && !e.altKey) {
      e.preventDefault();
      const start = ta.value.lastIndexOf('\n', ta.selectionStart - 1) + 1;
      const lead = /^[ \t]*/.exec(ta.value.slice(start, ta.selectionStart))?.[0] ?? '';
      const opens = ta.value.slice(start, ta.selectionStart).trimEnd().endsWith('{');
      insert(ta, `\n${lead}${opens ? INDENT : ''}`, onChange);
      return;
    }
    if (e.key !== 'Shift') tabReleased.current = false;
  };

  return (
    <section className="pg-editor" ref={box}>
      <pre className="pg-gutter" aria-hidden="true">
        {lines.map((_, i) => (
          <span key={i} className={errorLines.has(i + 1) ? 'pg-ln pg-ln-err' : 'pg-ln'}>
            {i + 1}
          </span>
        ))}
      </pre>
      <section className="pg-code">
        <pre className="pg-hl" aria-hidden="true">
          <code>
            {lines.map((line, i) => (
              <span key={i} className={errorLines.has(i + 1) ? 'pg-line pg-line-err' : 'pg-line'}>
                {highlightQx(line)}
              </span>
            ))}
          </code>
        </pre>
        <textarea
          id={id}
          ref={textareaRef}
          className="pg-input"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={onKeyDown}
          onKeyUp={revealCaret}
          onInput={revealCaret}
          onClick={revealCaret}
          onBlur={() => (tabReleased.current = false)}
          aria-describedby={describedBy}
          aria-invalid={errorLines.size > 0 || undefined}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          autoComplete="off"
          wrap="off"
        />
      </section>
    </section>
  );
}
