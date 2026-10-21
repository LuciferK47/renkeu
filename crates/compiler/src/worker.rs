//! The compile worker: Tectonic runs in a child process so it can be killed.
//!
//! Tectonic holds a process-wide lock for the whole of a compile and offers
//! no way to interrupt the engine, so a document such as `\def\a{\a}\a`
//! would wedge an in-process compile — and every later one — for good.
//! A separate process can simply be killed (masterplan §15.1).
//!
//! The binary re-executes itself as `<exe> __compile-worker`; the first
//! statement of `main` calls [`run_if_requested`], before any runtime or GUI
//! starts. The parent writes one JSON [`WorkerRequest`] to stdin; the worker
//! writes its artefacts and, last, `result.json` into `out_dir`. A worker
//! that dies leaves no `result.json`, which is how a crash is told apart
//! from a TeX error (which is an ordinary result).

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;

/// Argument that switches the binary into worker mode.
pub const WORKER_ARG: &str = "__compile-worker";

pub(crate) const PDF: &str = "output.pdf";
pub(crate) const SYNCTEX: &str = "output.synctex.gz";
pub(crate) const LOG: &str = "output.log";
pub(crate) const BLG: &str = "output.blg";
pub(crate) const BBL: &str = "output.bbl";
pub(crate) const RESULT: &str = "result.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRequest {
    /// Canonical project root.
    pub root: PathBuf,
    /// Main file, relative to `root`.
    pub main: String,
    pub synctex: bool,
    /// Directory the worker writes into; must exist.
    pub out_dir: PathBuf,
    /// Files (relative to `root`) the engine must not be able to read.
    pub hidden: Vec<PathBuf>,
    /// Bundle location override (see `CompileJob::bundle`).
    #[serde(default)]
    pub bundle: Option<String>,
}

// Independent facts on the wire (which artefacts exist), not a state machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerResult {
    /// The engine ran to completion without error.
    pub tex_ok: bool,
    /// Why it did not, in the engine's words.
    pub engine_error: Option<String>,
    pub has_pdf: bool,
    pub has_synctex: bool,
    pub has_log: bool,
    pub has_blg: bool,
    /// BibTeX ran and wrote a `.bbl`.
    #[serde(default)]
    pub has_bbl: bool,
}

/// If this process was started as a compile worker, run as one and never
/// return; otherwise do nothing. Call it first thing in `main`.
pub fn run_if_requested() {
    if std::env::args().nth(1).as_deref() != Some(WORKER_ARG) {
        return;
    }
    let code = match worker_main() {
        Ok(()) => 0,
        Err(message) => {
            let _ = writeln!(std::io::stderr(), "compile worker: {message}");
            1
        }
    };
    std::process::exit(code);
}

fn worker_main() -> Result<(), String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("reading the request: {e}"))?;
    let request: WorkerRequest =
        serde_json::from_str(&input).map_err(|e| format!("bad request: {e}"))?;

    let result = crate::engine::run(&request);

    // Written last and renamed into place, so its presence means the other
    // files are complete.
    let json = serde_json::to_vec(&result).map_err(|e| e.to_string())?;
    let tmp = request.out_dir.join(format!("{RESULT}.tmp"));
    std::fs::write(&tmp, json).map_err(|e| format!("writing the result: {e}"))?;
    std::fs::rename(&tmp, request.out_dir.join(RESULT))
        .map_err(|e| format!("publishing the result: {e}"))
}
