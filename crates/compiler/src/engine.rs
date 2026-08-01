//! The raw, blocking Tectonic invocation.
//!
//! `tectonic::latex_to_pdf` is a convenience wrapper that cannot emit `SyncTeX`,
//! so we drive `ProcessingSessionBuilder` directly with `synctex(true)` and
//! collect BOTH artefacts from the in-memory file map — no `Tectonic.toml`, no
//! output directory, no temp files (masterplan.md §12 M3).

use crate::CompileError;
use tectonic::driver::{OutputFormat, ProcessingSessionBuilder};
use tectonic::status::NoopStatusBackend;

/// What one engine run produced.
pub(crate) struct RawOutput {
    pub(crate) pdf: Vec<u8>,
    /// gzipped `SyncTeX` map; `None` when `SyncTeX` was not requested or the
    /// engine produced none.
    pub(crate) synctex_gz: Option<Vec<u8>>,
}

/// Run Tectonic synchronously. Blocking and NOT thread-safe — callers must
/// hold the process-wide engine mutex and run this on a blocking thread.
///
/// `tex_name` is the name the engine gives the primary input. It matters:
/// `SyncTeX` records it as the source file, and the viewer maps hits back
/// through it.
pub(crate) fn run(
    latex: &str,
    tex_name: &str,
    want_synctex: bool,
) -> Result<RawOutput, CompileError> {
    let mut status = NoopStatusBackend::default();

    let config = tectonic::config::PersistentConfig::open(false)
        .map_err(|e| CompileError::Tectonic(format!("opening tectonic config: {e}")))?;
    let bundle = config
        .default_bundle(false)
        .map_err(|e| CompileError::Tectonic(format!("loading resource bundle: {e}")))?;
    let format_cache_path = config
        .format_cache_path()
        .map_err(|e| CompileError::Tectonic(format!("format cache: {e}")))?;

    let mut files = {
        let mut builder = ProcessingSessionBuilder::default();
        builder
            .bundle(bundle)
            .primary_input_buffer(latex.as_bytes())
            .tex_input_name(tex_name)
            .format_name("latex")
            .format_cache_path(format_cache_path)
            .keep_logs(false)
            .keep_intermediates(false)
            .print_stdout(false)
            .synctex(want_synctex)
            .output_format(OutputFormat::Pdf)
            .do_not_write_output_files();

        let mut session = builder
            .create(&mut status)
            .map_err(|e| CompileError::Tectonic(format!("creating session: {e}")))?;
        session
            .run(&mut status)
            .map_err(|e| CompileError::Tectonic(format!("the LaTeX engine failed: {e}")))?;
        session.into_file_data()
    };

    // Artefacts are keyed by the input stem, e.g. `main.tex` -> `main.pdf`.
    let stem = tex_name.strip_suffix(".tex").unwrap_or(tex_name);

    let pdf = files
        .remove(&format!("{stem}.pdf"))
        .map(|f| f.data)
        .ok_or_else(|| {
            CompileError::Tectonic("engine reported success but produced no PDF".to_owned())
        })?;

    let synctex_gz = files.remove(&format!("{stem}.synctex.gz")).map(|f| f.data);

    Ok(RawOutput { pdf, synctex_gz })
}
