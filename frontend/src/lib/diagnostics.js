// Turn the server's diagnostics (compiled-event payload / build manifest) into
// the lists the Problems panel shows.

const shape = (d) => ({
  message: d.message,
  file: d.file ?? null,
  line: d.line ?? null,
  context: d.context ?? null,
  kind: d.kind
});

/** Split diagnostics into errors and warnings, keeping file and line. */
export function splitDiagnostics(diagnostics) {
  const list = Array.isArray(diagnostics) ? diagnostics : [];
  return {
    errors: list.filter((d) => d.severity === 'error').map(shape),
    warnings: list.filter((d) => d.severity === 'warning').map(shape)
  };
}

/**
 * Editor markers for one file's diagnostics. Lines the file no longer has
 * (the log refers to the compiled text, not what was edited since) are
 * dropped rather than pinned to the last line.
 * @param {string} path
 * @param {{errors: object[], warnings: object[]}} found
 * @param {{ getLineCount(): number, getLineMaxColumn(n: number): number }} model
 * @param {{ Error: number, Warning: number }} severity Monaco's MarkerSeverity
 */
export function markersFor(path, { errors, warnings }, model, severity) {
  const lines = model.getLineCount();
  const make = (d, level) => ({
    severity: level,
    message: d.message,
    startLineNumber: d.line,
    endLineNumber: d.line,
    startColumn: 1,
    endColumn: model.getLineMaxColumn(d.line),
    source: 'LaTeX'
  });
  const mine = (d) => d.file === path && Number.isInteger(d.line) && d.line >= 1 && d.line <= lines;
  return [
    ...errors.filter(mine).map((d) => make(d, severity.Error)),
    ...warnings.filter(mine).map((d) => make(d, severity.Warning))
  ];
}
