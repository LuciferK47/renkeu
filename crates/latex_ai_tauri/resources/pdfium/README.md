The desktop bundle ships libpdfium from this folder. `scripts/fetch-pdfium.sh`
puts the library here before `cargo tauri build`; the downloaded files are not
committed (see .gitignore). Without one, the app falls back to a system
libpdfium and PDF preview needs it installed.
