# Renkeu alpha: tester guide

Thank you for trying it. This is an early build for Linux and Windows. It works
on real papers, and it has had very little use by anyone but its author, so
please expect rough edges and tell us about them.

## Install

- **Windows:** run `Renkeu_<version>_x64-setup.exe`. It is not code-signed, so
  Windows SmartScreen says "Windows protected your PC": choose *More info*, then
  *Run anyway*. Compare the file's SHA-256 with `SHA256SUMS` on the release page
  first (`certutil -hashfile <file> SHA256`).
- **Linux:** install the `.deb`, or make the `.AppImage` executable and run it.
- The first compile downloads the TeX support files (about 40 MB), so it needs a
  network connection once.

Open a folder that holds your paper (**Open project folder**). Renkeu never
changes a file until you press Apply.

## The task

Take the last paper you wrote that had a page limit, and do this:

1. Open it and compile. Check the page count in the status bar.
2. Open the **Layout** tab. How many lines is it over (or under) the limit?
3. Run **Fit the document to a page limit…** (press F1 and type "fit"), and set
   the limit.
4. Read what it proposes. Accept only what you would have written yourself.
5. Open the **Submission** tab, pick the profile that matches where you will
   send the paper, and read the report.

Without an API key everything except step 3 works. For step 3 you need either
an API key (Settings, Assistant) or a local Ollama model. A small local model on
a laptop without a GPU can take ten minutes per answer.

## What we would like to know

- Did it build your paper the same as your usual setup? If not, what differed?
- Fit: did it reach the limit? Did you accept the changes? Did it propose
  anything you did not want, or refuse something reasonable?
- Submission: did it flag something that was wrong, miss something, or flag
  something that was fine?
- Anything slow, confusing or broken.

Send it to the address in **Settings, Privacy**, and paste the **diagnostics
report** from that tab. The report has the version, system and settings. It has
no project text and no key. Please do not send your paper unless you want to.

## What it does not do yet

- Tagged (accessible) PDFs: Tectonic cannot make them. The accessibility
  profile only checks alt text and font embedding.
- Check a venue's rules for you: the profiles are plain text files you can read
  and edit (`latex-ai profiles` lists them), and a clean report means nothing was
  found, not that the paper will be accepted.
- Real-time collaboration, a hosted editor, or macOS builds.
- Update itself: to upgrade, install the next build over this one.
