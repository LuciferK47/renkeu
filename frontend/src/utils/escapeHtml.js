// Escape text before it is interpolated into an `{@html}` block. Chat text can
// contain model output or server error messages, which are attacker-influenced
// (a document can prompt-inject), so it must never reach the DOM as markup.
export function escapeHtml(text) {
  return String(text ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}
