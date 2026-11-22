import {Fragment} from 'react';

/** Renders text with `backtick` spans as <code>; odd-indexed parts of the split are code. */
export function Inline({text}: {text: string}) {
  return (
    <>
      {text.split('`').map((part, i) => (i % 2 === 1 ? <code key={i}>{part}</code> : <Fragment key={i}>{part}</Fragment>))}
    </>
  );
}
