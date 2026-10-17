//! The blocking Tectonic invocation, run inside the worker process.
//!
//! Everything is kept in memory (`do_not_write_output_files`): a document
//! that does `\openout` to an absolute path writes into Tectonic's in-memory
//! file map, not onto the disk (verified in the M10 spike and pinned by a
//! test). Only the artefacts we choose are written, into `out_dir`.

use crate::worker::{BLG, LOG, PDF, SYNCTEX, WorkerRequest, WorkerResult};
use std::path::Path;
use tectonic::driver::{OutputFormat, ProcessingSessionBuilder};
use tectonic::status::{MessageKind, StatusBackend};

/// Remembers the engine's own error messages, for when there is no log.
#[derive(Default)]
struct Collect {
    errors: Vec<String>,
}

impl StatusBackend for Collect {
    fn report(&mut self, kind: MessageKind, args: std::fmt::Arguments, err: Option<&anyhow::Error>) {
        if matches!(kind, MessageKind::Error) && self.errors.len() < 20 {
            let mut line = args.to_string();
            if let Some(e) = err {
                line.push_str(&format!(": {e}"));
            }
            self.errors.push(line);
        }
    }

    fn dump_error_logs(&mut self, _output: &[u8]) {}
}

fn failed(error: String) -> WorkerResult {
    WorkerResult {
        tex_ok: false,
        engine_error: Some(error),
        has_pdf: false,
        has_synctex: false,
        has_log: false,
        has_blg: false,
    }
}

pub(crate) fn run(req: &WorkerRequest) -> WorkerResult {
    let mut status = Collect::default();

    let main_path = req.root.join(&req.main);
    let Some(main_name) = main_path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return failed(format!("`{}` is not a file name", req.main));
    };
    let main_dir = main_path.parent().unwrap_or(&req.root).to_path_buf();

    let config = match tectonic::config::PersistentConfig::open(false) {
        Ok(c) => c,
        Err(e) => return failed(format!("opening the Tectonic config: {e}")),
    };
    let bundle = match config.default_bundle(false) {
        Ok(b) => b,
        Err(e) => return failed(format!("loading the resource bundle: {e}")),
    };
    let format_cache_path = match config.format_cache_path() {
        Ok(p) => p,
        Err(e) => return failed(format!("opening the format cache: {e}")),
    };

    let (run, mut files) = {
        let mut builder = ProcessingSessionBuilder::default();
        builder
            .bundle(bundle)
            .primary_input_path(&main_path)
            .tex_input_name(&main_name)
            .format_name("latex")
            .format_cache_path(format_cache_path)
            .keep_logs(true)
            .keep_intermediates(false)
            .print_stdout(false)
            .synctex(req.synctex)
            .output_format(OutputFormat::Pdf)
            .do_not_write_output_files();
        // Textual match against `<main dir>/<name>`: it stops `\input{.env}`
        // and friends, not a path spelled around the check.
        for rel in &req.hidden {
            if let Ok(inside_main_dir) = req.root.join(rel).strip_prefix(&main_dir) {
                builder.hide(main_dir.join(inside_main_dir));
            }
        }

        let mut session = match builder.create(&mut status) {
            Ok(s) => s,
            Err(e) => return failed(format!("creating the session: {e}")),
        };
        let run = session.run(&mut status);
        let files = session.into_file_data();
        (run, files)
    };

    let stem = main_name.strip_suffix(".tex").unwrap_or(&main_name);
    let mut take = |ext: &str, out_name: &str| -> bool {
        match files.remove(&format!("{stem}.{ext}")) {
            Some(f) if !f.data.is_empty() => write_artefact(&req.out_dir, out_name, &f.data),
            _ => false,
        }
    };
    let has_pdf = take("pdf", PDF);
    let has_synctex = take("synctex.gz", SYNCTEX);
    let has_log = take("log", LOG);
    let has_blg = take("blg", BLG);

    let engine_error = run
        .err()
        .map(|e| format!("{e}"))
        .map(|e| match status.errors.first() {
            Some(first) => format!("{e} ({first})"),
            None => e,
        });
    WorkerResult {
        tex_ok: engine_error.is_none(),
        engine_error,
        has_pdf,
        has_synctex,
        has_log,
        has_blg,
    }
}

fn write_artefact(dir: &Path, name: &str, data: &[u8]) -> bool {
    match std::fs::write(dir.join(name), data) {
        Ok(()) => true,
        Err(e) => {
            let _ = std::io::Write::write_fmt(
                &mut std::io::stderr(),
                format_args!("compile worker: writing {name}: {e}\n"),
            );
            false
        }
    }
}
