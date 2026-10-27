# Page-fit benchmark

Does the "fit to N pages" assistant work on real papers, with a real model, and
does it keep to the rules? This folder builds the test and runs it.

## The papers

`manifest.json` lists ten arXiv papers. Each is there because arXiv's own
record for it says **CC BY 4.0** (the OAI-PMH `license` field), it builds with
Renkeu's engine, and it is 5 to 16 pages long. Their sources are downloaded to
`corpus/` by `fetch.py` and are **not** committed: the papers stay with their
authors. Credit: see the ids and titles in the manifest; the arXiv abstract page
for each id names the authors.

## The task

Each paper is asked to lose **one page**. Only papers where that costs between
1 and 30 lines of text are used (`lines_over` in the manifest), because a
paper whose last page is full needs a whole page of cuts, which no amount of
tightening wording can give. The papers were not padded; their natural length
sets the task.

## What is checked, and by whom

`run.py` starts a server on a copy of the paper, runs the assistant with
`auto_apply`, then judges the result **without trusting the server's own
guards**:

- pages, from a fresh `latex-ai check`, and whether it still builds;
- sections, figures, tables, display equations, labels and citation keys before
  and after, counted with separate patterns from the ones in the server;
- space-buying tricks in the added lines (negative `\vspace`, margin, font and
  spacing changes, class changes), again with its own list;
- how much was written (letters and digits outside comments) and how many
  lines changed.

A paper counts as **fit** only if the limit is met, nothing was lost and no
trick was added.

## Running it

```bash
python3 bench/fit/fetch.py --bin target/debug/latex-ai --want 10
ollama create qwen2.5:3b-8k -f bench/fit/Modelfile
python3 bench/fit/run.py --model qwen2.5:3b-8k
```

Results go to `results/` (git-ignored): one folder per paper with the event
log, the final files and `result.json`, and `summary.md`.

## The model

The assistant calls whatever model is configured. This run used a small local
one through Ollama, on a laptop CPU with no GPU, because that is the machine at
hand. A 7B model could not answer within the 600 s limit a request may be given
(about 5 prompt tokens a second here), so it was not used. Results with a
better model will differ; read them as a floor, not a ceiling.
