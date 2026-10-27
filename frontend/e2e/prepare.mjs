// Run once before the server starts: an empty data directory and the fixture workspace.
import fs from 'node:fs'
import { HOME, resetWorkspace } from './fixture.js'

fs.rmSync(HOME, { recursive: true, force: true })
fs.mkdirSync(HOME, { recursive: true })
// The welcome screen is shown once per data directory; the tests start past it.
fs.writeFileSync(`${HOME}/first-run.done`, '')
resetWorkspace()
