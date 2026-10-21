# synctex

A reader for `SyncTeX` files (`.synctex` / `.synctex.gz`) that maps between PDF
coordinates and source positions, in pure safe Rust.

- **Backward search:** a point on a PDF page to the source file and line that
  produced it, preferring the tightest box that contains the point.
- **Forward search:** a source line to the regions of the PDF it produced.
- Lengths are converted from TeX scaled points to PDF points (bp), with the page
  origin at the top-left.

```rust,no_run
use synctex::SyncTexData;
use std::path::Path;

let data = SyncTexData::parse(Path::new("output.synctex.gz"))?;
// Click at (140 bp, 220 bp) on page 1:
if let Some(hit) = data.backward_search(1, 140.0, 220.0) {
    println!("{}:{}", hit.file, hit.line);
}
// Where did line 12 of the primary input land?
if let Some(tag) = data.primary_tag() {
    for region in data.forward_search(tag, 12) {
        println!("page {} at ({}, {})", region.page, region.x, region.y);
    }
}
# Ok::<(), synctex::SyncTexError>(())
```

The crate only parses the format; it knows nothing about LaTeX. It was written
for [Renkeu](https://github.com/seriouslysahid/Latex-AI) and is tested against
real Tectonic output.

Licensed under either of Apache-2.0 or MIT at your option.
