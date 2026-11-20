// The programs in examples/qx/, bundled at build time so the list always matches the repository.
const files = import.meta.glob('../../../examples/qx/*.qx', {query: '?raw', import: 'default', eager: true}) as Record<
  string,
  string
>;

export type Example = {file: string; code: string};

// Same order as the Examples page; files added later follow alphabetically.
const ORDER = ['bell.qx', 'ghz.qx', 'teleport.qx', 'functions.qx', 'rotation.qx', 'teleport_fn.qx', 'conditional_fn.qx'];
const rank = (f: string) => {
  const i = ORDER.indexOf(f);
  return i < 0 ? ORDER.length : i;
};

export const EXAMPLES: Example[] = Object.entries(files)
  .map(([path, code]) => ({file: path.slice(path.lastIndexOf('/') + 1), code}))
  .sort((a, b) => rank(a.file) - rank(b.file) || a.file.localeCompare(b.file));
