// BibTeX entries, for citation completion. Finds `@type{key, field = {…}, …}`
// and the few fields worth showing; `@string`, `@comment` and `@preamble` are
// not entries.

import { balancedGroup } from './scan.js';

const NOT_ENTRIES = new Set(['string', 'comment', 'preamble']);
const HEAD = /@([A-Za-z]+)\s*[{(]\s*([^,\s{}()]+)\s*,/g;

/** The value of `field = …` inside one entry body, braces/quotes removed. */
function fieldValue(body, field) {
  const re = new RegExp(`(?:^|[\\s,])${field}\\s*=\\s*`, 'i');
  const m = re.exec(body);
  if (!m) return '';
  const at = m.index + m[0].length;
  let value = '';
  if (body[at] === '{') value = balancedGroup(body, at) ?? '';
  else if (body[at] === '"') {
    const end = body.indexOf('"', at + 1);
    value = end > at ? body.slice(at + 1, end) : '';
  } else value = /^[^,\s}]*/.exec(body.slice(at))?.[0] ?? '';
  return value.replace(/[{}]/g, '').replace(/\s+/g, ' ').trim();
}

/** @returns {{key:string, type:string, line:number, title:string, author:string, year:string}[]} */
export function scanBib(text) {
  const entries = [];
  HEAD.lastIndex = 0;
  for (let m = HEAD.exec(text); m; m = HEAD.exec(text)) {
    const type = m[1].toLowerCase();
    if (NOT_ENTRIES.has(type)) continue;
    // The body runs to the next entry head; field values rarely contain '@'.
    const rest = text.slice(HEAD.lastIndex);
    const next = rest.search(/\n\s*@[A-Za-z]+\s*[{(]/);
    const body = next === -1 ? rest : rest.slice(0, next);
    entries.push({
      key: m[2],
      type,
      line: text.slice(0, m.index).split('\n').length,
      title: fieldValue(body, 'title'),
      author: fieldValue(body, 'author'),
      year: fieldValue(body, 'year') || fieldValue(body, 'date').slice(0, 4)
    });
  }
  return entries;
}
