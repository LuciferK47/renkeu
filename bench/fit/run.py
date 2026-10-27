#!/usr/bin/env python3
"""Run the page-fit agent against the benchmark corpus with a real model.

For each paper in manifest.json: copy it, start a server on the copy, ask the
assistant to bring it one page under its length, then judge the outcome with
checks that do not trust the server's own guards:

  * the page count of the result, from a fresh `latex-ai check`;
  * whether the result still builds;
  * sections, figures, tables, display equations, labels and citation keys
    before and after, counted here with separate patterns;
  * whether any added line is a space-buying trick (own list, own patterns);
  * how much text changed.

    python3 bench/fit/run.py --model qwen2.5:7b-32k --only 2403.00692

Results go to bench/fit/results/ (git-ignored); summary.md is the report.
"""
import argparse, difflib, json, os, re, shutil, socket, subprocess, sys, time, urllib.request
from pathlib import Path

HERE = Path(__file__).parent
ROOT = HERE.parent.parent
CORPUS, RESULTS = HERE / "corpus", HERE / "results"

TRICKS = [
    r"\\vspace\*?\s*\{\s*-", r"\\[vh]skip\s*-", r"\\\\\*?\s*\[\s*-", r"\\(?:setlength|addtolength)",
    r"\\(?:newgeometry|geometry)\b", r"\\usepackage[^\n]*\{[^}]*(?:geometry|fullpage|savetrees|setspace|titlesec|enumitem)",
    r"\\(?:linespread|baselinestretch|fontsize|selectfont|looseness|enlargethispage)\b",
    r"\\(?:small|footnotesize|scriptsize|tiny)\b", r"\\documentclass\b",
]


def strip_comments(text):
    return "\n".join(re.split(r"(?<!\\)%", l)[0] for l in text.splitlines())


def tex_files(folder):
    return {p.relative_to(folder).as_posix(): p.read_text(errors="replace") for p in sorted(folder.rglob("*.tex"))}


def inventory(files):
    text = "\n".join(strip_comments(t) for t in files.values())
    keys = lambda pat: {k.strip() for m in re.finditer(pat, text) for k in m.group(1).split(",") if k.strip()}
    return {
        "sections": len(re.findall(r"\\(?:part|chapter|section|subsection|subsubsection)\*?\s*[\[{]", text)),
        "figures": len(re.findall(r"\\includegraphics\b|\\begin\{(?:tikzpicture|pgfpicture)\}", text)),
        "tables": len(re.findall(r"\\begin\{(?:tabular\*?|tabularx|longtable|array)\}", text)),
        "equations": len(re.findall(r"\\begin\{(?:equation|align|gather|multline|eqnarray|flalign|alignat)\*?\}|\\\[", text)) + text.count("$$") // 2,
        "labels": keys(r"\\label\s*\{([^}]*)\}"),
        "cites": keys(r"\\[A-Za-z]*cite[A-Za-z]*\*?\s*(?:\[[^\]]*\]\s*)*\{([^}]*)\}"),
    }


def lost(before, after):
    out = []
    for k in ("sections", "figures", "tables", "equations"):
        if after[k] < before[k]:
            out.append(f"{before[k] - after[k]} {k}")
    for k in ("labels", "cites"):
        gone = sorted(before[k] - after[k])
        if gone:
            out.append(f"{k}: {', '.join(gone[:5])}")
    return out


def tricks_added(before_files, after_files):
    found = []
    for name, new in after_files.items():
        old = set(before_files.get(name, "").splitlines())
        for line in new.splitlines():
            if line in old:
                continue
            live = strip_comments(line)
            for pat in TRICKS:
                if re.search(pat, live):
                    found.append(f"{name}: {line.strip()[:80]}")
                    break
    return found


def text_chars(files):
    """Letters and digits outside comments: a rough measure of how much was written."""
    return sum(1 for t in files.values() for c in strip_comments(t) if c.isalnum())


def changed_lines(before_files, after_files):
    n = 0
    for name in set(before_files) | set(after_files):
        a, b = before_files.get(name, "").splitlines(), after_files.get(name, "").splitlines()
        n += sum(1 for l in difflib.unified_diff(a, b, lineterm="", n=0) if l[:1] in "+-" and l[:3] not in ("+++", "---"))
    return n


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def call(port, method, path, body=None):
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", method=method, data=json.dumps(body).encode() if body is not None else None, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def check(binary, folder, pages=None):
    cmd = [binary, "check", str(folder), "--json"] + (["--pages", str(pages)] if pages else [])
    out = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
    return json.loads(out.stdout)


def stream(port, body, log, deadline):
    """POST the agent request; yield (seconds, event, data) as they arrive."""
    req = urllib.request.Request(f"http://127.0.0.1:{port}/api/agent/stream", data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    start, name = time.time(), "message"
    with urllib.request.urlopen(req, timeout=deadline) as r:
        for raw in r:
            line = raw.decode().rstrip("\n")
            if line.startswith("event: "):
                name = line[7:].strip()
            elif line.startswith("data: "):
                data = json.loads(line[6:])
                log.write(json.dumps({"t": round(time.time() - start, 1), "event": name, "data": data}) + "\n")
                log.flush()
                yield time.time() - start, name, data
                name = "message"


def run_one(entry, args):
    pid = entry["id"].replace("/", "_")
    out = RESULTS / pid
    shutil.rmtree(out, ignore_errors=True)
    work = out / "work"
    shutil.copytree(CORPUS / pid, work)
    before_files = tex_files(work)
    before = inventory(before_files)

    base = check(args.bin, work)
    pages = base["layout"]["pages"]
    ratio = base["layout"]["fill"]["ratio"]
    limit = pages - 1
    lines_over = check(args.bin, work, limit)["layout"]["budget"]["lines_over"]

    port = free_port()
    env = dict(os.environ, LATEX_AI_HOME=str(out / "home"))
    if args.pdfium:
        env["LATEX_AI_PDFIUM_DIR"] = args.pdfium
    server = subprocess.Popen([args.bin, "serve", "--port", str(port)], cwd=work, env=env, stdout=subprocess.DEVNULL, stderr=open(out / "server.log", "w"))
    result = {"id": entry["id"], "title": entry["title"], "pages_before": pages, "limit": limit, "lines_over": lines_over, "last_page_ratio": ratio, "model": args.model}
    try:
        for _ in range(100):
            try:
                call(port, "GET", "/health")
                break
            except OSError:
                time.sleep(0.3)
        call(port, "PUT", "/api/config/api-key", {"key": "placeholder-not-used-by-local-models"})
        call(port, "PATCH", "/api/config", {"model": args.model, "timeout_secs": args.call_timeout})
        events, final, started = [], {}, time.time()
        try:
            with open(out / "events.jsonl", "w") as log:
                for t, name, data in stream(port, {"file_path": base["main"], "pages": limit, "auto_apply": True, "max_iterations": args.attempts}, log, args.run_timeout):
                    events.append(name)
                    if name == "done":
                        final = data
        except Exception as e:  # noqa: BLE001
            final = {"success": False, "message": f"client error: {e}"}
        result["seconds"] = round(time.time() - started)
        result["attempts"] = events.count("thinking")
        result["refused"] = sum(1 for _ in open(out / "events.jsonl") if '"error_summary"' in _ and ('"layout": true' in _ or "refused" in _ or "unsafe" in _))
        result["server_said"] = {"success": final.get("success"), "message": final.get("message")}
    finally:
        server.terminate()
        server.wait(timeout=30)

    after_files = tex_files(work)
    after = check(args.bin, work, limit)
    result.update(
        pages_after=after["layout"]["pages"] if after.get("layout") else None,
        builds=bool(after.get("success")),
        reached=bool(after.get("success") and after["layout"]["pages"] <= limit),
        lost=lost(before, inventory(after_files)),
        tricks=tricks_added(before_files, after_files),
        lines_changed=changed_lines(before_files, after_files),
        text_kept=round(text_chars(after_files) / max(1, text_chars(before_files)), 3),
    )
    result["verdict"] = (
        "fit" if result["reached"] and not result["lost"] and not result["tricks"]
        else "fit-but-violates" if result["reached"] else "not-reached"
    )
    (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(f"{entry['id']}: {result['verdict']} {pages}->{result['pages_after']} (limit {limit}), {result['attempts']} attempts, {result['seconds']}s", flush=True)
    return result


def summarise(results, args):
    rows = ["| paper | pages | limit | lines over | after | attempts | time | text kept | lost | tricks | verdict |", "|---|---|---|---|---|---|---|---|---|---|---|"]
    for r in results:
        rows.append(f"| {r['id']} | {r['pages_before']} | {r['limit']} | {r['lines_over']} | {r['pages_after']} | {r['attempts']} | {r['seconds']}s | {r['text_kept']:.0%} | {len(r['lost'])} | {len(r['tricks'])} | {r['verdict']} |")
    fit = sum(r["verdict"] == "fit" for r in results)
    text = f"# Page-fit benchmark\n\nModel `{args.model}`, {len(results)} papers (arXiv, CC BY 4.0), each asked to lose one page, up to {args.attempts} attempts.\n\n" + "\n".join(rows) + f"\n\n**{fit} of {len(results)} fit cleanly** (limit met, nothing lost, no trick added).\n"
    (RESULTS / "summary.md").write_text(text)
    print(text)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=str(ROOT / "target/debug/latex-ai"))
    ap.add_argument("--model", default="qwen2.5:3b-8k")
    ap.add_argument("--attempts", type=int, default=5)
    ap.add_argument("--call-timeout", type=int, default=600)
    ap.add_argument("--run-timeout", type=int, default=3 * 3600)
    ap.add_argument("--pdfium", default=os.environ.get("LATEX_AI_PDFIUM_DIR"))
    ap.add_argument("--only", nargs="*")
    args = ap.parse_args()
    manifest = json.loads((HERE / "manifest.json").read_text())
    results = []
    for entry in manifest:
        if args.only and entry["id"] not in args.only:
            continue
        results.append(run_one(entry, args))
        summarise(results, args)


main()
