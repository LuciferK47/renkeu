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
