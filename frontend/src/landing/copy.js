// Every word on the landing page lives here, so it can be checked against what
// the product really does (tests/landingCopy.test.js) and edited in one place.
// The facts come from the code: what is sent to the provider is
// PlanRequest::render_prompt, the attempt limit is default_max_iterations, and
// the crates are the workspace members.

export const links = {
  github: 'https://github.com/LuciferK47/Latex-AI',
  releases: 'https://github.com/LuciferK47/Latex-AI/releases'
};

const commands = {
  docker: 'ALLOWED_ORIGINS=http://localhost:3000 docker compose up -d',
  source: 'cargo run -p latex-ai-server -- serve'
};

/** One label per action, everywhere. */
export const actions = {
  get: 'Get Renkeu',
  open: 'Open editor',
  source: 'View source on GitHub'
};

export const hero = {
  eyebrow: 'Local-first LaTeX',
  title: ['Write LaTeX.', 'Keep your files.', 'Fix the build.'],
  body:
    'An editor with the TeX engine built in, and an AI that repairs a broken build on a copy of your project, then waits for your review. Your files stay on your machine.',
  proof: ['no TeX Live install', 'no account', 'nothing changes until you apply it']
};

/** The four steps of the repair loop, each paired with a state of the scene. */
export const loop = {
  eyebrow: 'The repair loop',
  title: 'A broken build becomes a review, not a rescue mission.',
  body: 'Renkeu reads what the compiler actually reported, edits a copy of your project, recompiles it, and only then shows you the change.',
  footnote: 'If it cannot get a clean build in 3 attempts, it stops and your files stay exactly as they were.',
  steps: [
    {
      id: 'break',
      label: 'Break',
      title: 'The compiler says where.',
      body: 'A missing brace or an unknown command stops the build. The error arrives with a file and a line, not a wall of log.',
      diagnostic: '! Undefined control sequence.',
      where: 'main.tex:12'
    },
    {
      id: 'diagnose',
      label: 'Diagnose',
      title: 'It sends what it needs to reason.',
      body: 'The file you are working in goes to your provider, plus up to 3 other files the errors point to. Other files are named, never sent.',
      diagnostic: 'sent: main.tex (+ files the errors point to)',
      where: 'refs.bib, figures/ — names only'
    },
    {
      id: 'repair',
      label: 'Repair a copy',
      title: 'It tries the fix somewhere safe.',
      body: 'The edit is applied to a copy and the whole document is recompiled. Failed attempts feed their errors into the next one.',
      diagnostic: 'attempt 1 failed · attempt 2 compiled',
      where: 'your files: untouched'
    },
    {
      id: 'review',
      label: 'Review',
      title: 'You decide what lands.',
      body: 'Each change has its own checkbox. Press Apply to write the ones you chose; press Discard and nothing happens.',
      diagnostic: 'Apply 1 change',
      where: 'main.tex · line 12'
    }
  ]
};

export const workspace = {
  eyebrow: 'The workspace',
  title: 'Edit the source. Inspect the page. Jump between them without hunting.',
  body: 'Files on the left, source in the middle, the PDF beside it. Press Ctrl+Alt+J to see where a line landed on the page; double-click the page to go back to the line.',
  tabs: [
    { id: 'project', label: 'A real project', note: 'Many files: chapters, a .bib, figures, a chosen main document, \\input and \\include.' },
    { id: 'sync', label: 'Find line and jump back', note: 'Ctrl+Alt+J flashes the line on the page. Double-click the page to open its source.' },
    { id: 'review', label: 'Review a change', note: 'Every AI change is a diff with its own checkbox. Apply writes only what you chose.' }
  ]
};

export const capabilities = {
  eyebrow: 'What it does',
  title: 'Six tools. All of them run beside your files.',
  items: [
    { id: 'files', title: 'Multi-file projects', body: 'File tree, tabs, autosave, upload, rename, delete, and a main document of your choosing.' },
    { id: 'edit', title: 'LaTeX-aware editing', body: 'Completion for citations, labels and packages; an outline; spell check; go to definition on \\ref, \\cite and \\input.' },
    { id: 'sync', title: 'SyncTeX both ways', body: 'From a source line to its place on the page, and from a click on the page back to the source.' },
    { id: 'ai', title: 'Review-gated AI repair', body: 'Tries fixes on a copy, checks that the document compiles, then lets you apply them change by change.' },
    { id: 'visual', title: 'Visual fix', body: 'Draw a box on the PDF and say what is wrong. Reading the picture needs a vision-capable model; without one it works from the source.' },
    { id: 'history', title: 'History and undo', body: 'Every save and every applied change is recorded with what it replaced.' }
  ],
  planned: {
    label: 'Planned, not built yet',
    items: ['Project search and replace', 'Compile on save', 'Sidebar grouped by file type', 'An 8-bit editor theme']
  }
};

export const engine = {
  eyebrow: 'Rust inside',
  title: 'The compiler lives beside your project, not behind an account.',
  body: 'Rust holds the pipeline together: the compile runs in a worker that can be stopped, errors come back with a file and a line, and SyncTeX and page rendering happen in the same local program. No TeX Live, and no server you do not run.',
  note: 'Rust orchestrates; it does not do everything. Tectonic embeds the TeX engine, and the page renderer is pdfium, a C++ library driven from Rust.',
  crates: [
    { id: 'compiler', name: 'compiler', role: 'Drives Tectonic in a separate worker process, so a runaway document can be stopped. Turns the log into errors with files and lines. Caches builds by content.' },
    { id: 'core', name: 'latex_ai_core', role: 'The project sandbox, history in SQLite, the AI planner, and the edit engine behind reviewable changes.' },
    { id: 'server', name: 'latex_ai_server', role: 'The local HTTP server and the latex-ai command-line tool.' },
    { id: 'synctex', name: 'synctex', role: 'Reads SyncTeX data to map source lines to page positions and back.' },
    { id: 'visual', name: 'visual', role: 'Renders PDF pages with pdfium, a C++ library, through Rust bindings.' },
    { id: 'tauri', name: 'latex_ai_tauri', role: 'The desktop shell that embeds the server and the editor.' }
  ]
};

export const boundary = {
  eyebrow: 'The boundary',
  title: 'Your project is not our product.',
  body: 'Editing, compiling, history and page rendering happen on your machine. Only when you ask the AI does anything cross the boundary.',
  sent: [
    'your instruction',
    'the file you are working in, with line numbers',
    'up to 3 other files the compiler errors point to',
    'the names of your other files',
    'the compiler errors'
  ],
  sentVisual: 'For a visual fix: a crop of the page, if a vision model is configured.',
  local: 'With a local model such as Ollama, nothing leaves your machine.',
  small: 'This describes how the app is built to behave. The source is open, so you can check it.'
};

export const install = {
  eyebrow: 'Install',
  title: 'Three ways to run it.',
  items: [
    {
      id: 'docker',
      name: 'Docker',
      note: 'A reproducible local server.',
      command: commands.docker
    },
    {
      id: 'source',
      name: 'From source',
      note: 'For contributors and early users. Then run the editor with npm run dev in frontend/.',
      command: commands.source
    },
    {
      id: 'desktop',
      name: 'Desktop',
      note: 'The first release is coming. The Linux .deb is built and tested in CI; macOS and Windows are configured but untested.',
      cta: 'Watch releases'
    }
  ]
};

export const faq = {
  eyebrow: 'Questions',
  title: 'What runs where, and what changes only when you say so.',
  items: [
    ['Do I need TeX Live?', 'No. The Tectonic engine is built in. Packages a document needs are downloaded the first time it is compiled, then cached.'],
    ['Does it work offline?', 'Editing and compiling do, once the packages are cached. The first compile of a document needs a network connection. The AI needs a provider; a local model such as Ollama works offline.'],
    ['What exactly is sent to the AI?', 'Only when you ask it: your instruction, the file you are working in (with line numbers), up to 3 other files the compiler errors point to, the names of your other files, and the compiler errors. Nothing is sent when you do not use the AI.'],
    ['Can I use my own or a local model?', 'Yes. Any model the genai library can reach, including local Ollama models. Small models may not find a fix; larger ones do better.'],
    ['What if the AI cannot fix my document?', 'It stops after 3 attempts and your files stay exactly as they were.'],
    ['Can it change my files without asking?', 'No. Changes are written only when you press Apply, and only the ones you ticked.'],
    ['Is there a hosted version or collaboration?', 'No. There are no accounts, no hosted projects and no real-time editing.'],
    ['What is the licence?', 'MIT or Apache-2.0, at your option.']
  ]
};

export const closing = {
  title: ['Compile locally.', 'Repair deliberately.', 'Keep the project yours.']
};

/** The crew: scenery with jobs. Names and roles only; none of this is a feature. */
export const crew = {
  bruno: { name: 'Bruno', line: 'greets you from the roof and points the way down.' },
  repair: { name: 'The courier and the repair bot', line: 'The crab carries the diagnostic; the bot tries the fix on a copy. Neither touches your files until you press Apply.' },
  hazel: { name: 'Hazel', line: 'works the editor and the page: from source to PDF and back.' },
  ember: { name: 'Ember', line: 'runs the compile bench: build, errors, page.' },
  aurum: { name: 'Aurum', line: 'guards the vault and wakes only when you ask the AI.' },
  pip: { name: 'Pip', line: 'hands out the packages.' }
};

export const footer = 'Built with Rust, Tectonic and Svelte. MIT or Apache-2.0.';

/** The floors of the tower, in scroll order. Shared by the scene and the nav. */
export const floors = [
  { id: 'hero', label: 'STREET' },
  { id: 'loop', label: 'FL 04 · REPAIR' },
  { id: 'workspace', label: 'FL 03 · WORKSPACE' },
  { id: 'engine', label: 'FL 02 · ENGINE' },
  { id: 'boundary', label: 'FL 01 · VAULT' },
  { id: 'install', label: 'DOCK' },
  { id: 'closing', label: 'STREET' }
];
