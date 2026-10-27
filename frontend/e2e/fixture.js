// The workspace the server runs in, and putting it back the way it started.
import fs from 'node:fs'
import path from 'node:path'

const WORKSPACE = path.resolve(import.meta.dirname, '.workspace')
export const HOME = path.resolve(import.meta.dirname, '.home')

const FILES = {
  'main.tex': [
    '\\documentclass{article}',
    '\\begin{document}',
    '\\section{Introduction}\\label{sec:intro}',
    'The quick brown fox jumps over the lazy dog, as shown in Section~\\ref{sec:intro}.',
    '',
    '\\input{chapters/one}',
    '',
    'Closing words.',
    '\\end{document}',
    ''
  ].join('\n'),
  'chapters/one.tex': '\\section{Chapter one}\\label{sec:one}\nText that lives in the first chapter.\n',
  'refs.bib': '@article{knuth84,\n  author = {Donald E. Knuth},\n  title = {Literate Programming},\n  year = {1984}\n}\n'
}

/** Remove everything in the workspace and write the starting files again. */
export function resetWorkspace() {
  fs.mkdirSync(WORKSPACE, { recursive: true })
  for (const entry of fs.readdirSync(WORKSPACE)) fs.rmSync(path.join(WORKSPACE, entry), { recursive: true, force: true })
  for (const [name, text] of Object.entries(FILES)) {
    const file = path.join(WORKSPACE, name)
    fs.mkdirSync(path.dirname(file), { recursive: true })
    fs.writeFileSync(file, text)
  }
}

export const read = (name) => fs.readFileSync(path.join(WORKSPACE, name), 'utf8')
export const exists = (name) => fs.existsSync(path.join(WORKSPACE, name))
export const write = (name, text) => fs.writeFileSync(path.join(WORKSPACE, name), text)

/** Remove everything, so the project is empty (the state a template starts from). */
export function emptyWorkspace() {
  fs.mkdirSync(WORKSPACE, { recursive: true })
  for (const entry of fs.readdirSync(WORKSPACE)) fs.rmSync(path.join(WORKSPACE, entry), { recursive: true, force: true })
}
