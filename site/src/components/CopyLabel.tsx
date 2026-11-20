/** Button label that swaps "Copy" for an animated check mark; fixed min-width (see .copy) keeps the layout still. */
export function CopyLabel({copied}: {copied: boolean}) {
  return copied ? (
    <span className="copy-label copy-done">
      <svg className="copy-check" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
        <path d="M3 8.5l3.2 3L13 4.5" pathLength={1} />
      </svg>
      Copied
    </span>
  ) : (
    <span className="copy-label">Copy</span>
  );
}
