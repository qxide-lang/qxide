// Whether the `qxide lsp` that answered is new enough for the `.qx` syntax this extension
// documents. Kept free of `vscode` imports so `node --test` can load it.

/// First release whose language server knows the general-purpose syntax (`while`, arrays, ...).
export const MIN_VERSION = "0.9.0";
/// `capabilities.experimental.qxideLanguageLevel` a server must report (src/lsp/mod.rs). Builds
/// between releases report it while their version still names the previous release.
export const MIN_LANGUAGE_LEVEL = 1;

export interface ServerReport {
  version?: string;
  languageLevel?: unknown;
}

/// `[major, minor, patch]` of `0.8.0` or `0.9.0-dev`; `undefined` for anything else.
export function parseVersion(v: string | undefined): number[] | undefined {
  const m = /^(\d+)\.(\d+)\.(\d+)/.exec((v ?? "").trim());
  return m === null ? undefined : [Number(m[1]), Number(m[2]), Number(m[3])];
}

/// True only when the server is known to be older than this extension expects: it reports no
/// sufficient language level and a version below `MIN_VERSION`. An unknown version is not reported.
export function isTooOld(report: ServerReport): boolean {
  const level = report.languageLevel;
  if (typeof level === "number" && level >= MIN_LANGUAGE_LEVEL) {
    return false;
  }
  const have = parseVersion(report.version);
  const min = parseVersion(MIN_VERSION) as number[];
  if (have === undefined) {
    return false;
  }
  for (let i = 0; i < 3; i++) {
    if (have[i] !== min[i]) {
      return have[i] < min[i];
    }
  }
  return false;
}
