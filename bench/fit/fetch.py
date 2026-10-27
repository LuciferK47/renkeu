#!/usr/bin/env python3
"""Build the page-fit benchmark corpus from arXiv papers licensed CC BY.

Only papers whose arXiv record says CC BY 4.0 are used. Their sources are
downloaded into bench/fit/corpus/ (git-ignored: the papers stay with their
authors, and only manifest.json, the ids and licence, is committed). A paper
is kept if Renkeu's engine builds it to a plausible length.

    python3 bench/fit/fetch.py --bin target/debug/latex-ai --want 10

Be polite to arXiv: one request every few seconds.
"""
import argparse, gzip, io, json, random, re, subprocess, sys, tarfile, time, urllib.request
from pathlib import Path
from xml.etree import ElementTree as ET

HERE = Path(__file__).parent
CORPUS = HERE / "corpus"
OAI = "https://oaipmh.arxiv.org/oai?verb=ListRecords&set=cs&metadataPrefix=arXiv&from={d}&until={d}"
NS = {"a": "http://arxiv.org/OAI/arXiv/"}
UA = {"User-Agent": "renkeu-bench/0.1 (research; contact: f20230436@goa.bits-pilani.ac.in)"}
MAX_BYTES = 8_000_000


def get(url, timeout=120):
    time.sleep(3)
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=timeout) as r:
        return r.read()


def cc_by_records(day):
    root = ET.fromstring(get(OAI.format(d=day)))
    for rec in root.iter("{http://www.openarchives.org/OAI/2.0/}record"):
        meta = rec.find(".//a:arXiv", NS)
        if meta is None:
            continue
        lic = (meta.findtext("a:license", "", NS) or "")
        if "/licenses/by/4.0" in lic:
            yield meta.findtext("a:id", "", NS), " ".join((meta.findtext("a:title", "", NS) or "").split()), lic


def unpack(blob, dest):
    dest.mkdir(parents=True, exist_ok=True)
    try:
        with tarfile.open(fileobj=io.BytesIO(blob)) as t:
            for m in t.getmembers():
                p = (dest / m.name).resolve()
                if not m.isfile() or not str(p).startswith(str(dest.resolve())):
                    continue
                t.extract(m, dest, filter="data")
        return True
    except tarfile.TarError:
        try:
            (dest / "main.tex").write_bytes(gzip.decompress(blob))
            return True
        except OSError:
            return False


def check(binary, folder):
    out = subprocess.run([binary, "check", str(folder), "--json"], capture_output=True, text=True, timeout=300)
    try:
        return json.loads(out.stdout)
    except json.JSONDecodeError:
        return None


def lines_over(binary, folder, pages):
    """Lines of text past the end of page `pages - 1`: what one page less costs."""
    out = subprocess.run([binary, "check", str(folder), "--pages", str(pages - 1), "--json"], capture_output=True, text=True, timeout=300)
    try:
        return json.loads(out.stdout)["layout"]["budget"]["lines_over"]
    except (json.JSONDecodeError, KeyError, TypeError):
        return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default="target/debug/latex-ai")
    ap.add_argument("--want", type=int, default=10)
    ap.add_argument("--days", nargs="+", default=["2024-03-05", "2024-03-12", "2024-03-19"])
    ap.add_argument("--min-pages", type=int, default=5)
    ap.add_argument("--max-pages", type=int, default=16)
    ap.add_argument("--max-over", type=int, default=30, help="most lines one page less may cost (a whole page is not a fair task)")
    a = ap.parse_args()

    CORPUS.mkdir(exist_ok=True)
    manifest_path = HERE / "manifest.json"
    kept = json.loads(manifest_path.read_text()) if manifest_path.exists() else []
    # Judge what is already kept by the same rule, so the corpus is one kind of task.
    for k in list(kept):
        k["lines_over"] = lines_over(a.bin, CORPUS / k["id"].replace("/", "_"), k["pages"])
        if k["lines_over"] is None or not 1 <= k["lines_over"] <= a.max_over:
            kept.remove(k)
            subprocess.run(["rm", "-rf", str(CORPUS / k["id"].replace("/", "_"))])
    manifest_path.write_text(json.dumps(kept, indent=2) + "\n")
    have = {k["id"] for k in kept}
    seen = set(json.loads((HERE / "seen.json").read_text())) if (HERE / "seen.json").exists() else set()
    pool = [r for d in a.days for r in cc_by_records(d)]
    random.Random(7).shuffle(pool)
    print(f"{len(pool)} CC BY candidates", file=sys.stderr)

    for pid, title, lic in pool:
        if len(kept) >= a.want:
            break
        if pid in have or pid in seen:
            continue
        seen.add(pid)
        (HERE / "seen.json").write_text(json.dumps(sorted(seen)))
        folder = CORPUS / pid.replace("/", "_")
        try:
            blob = get(f"https://arxiv.org/e-print/{pid}")
            if len(blob) > MAX_BYTES or not unpack(blob, folder):
                raise ValueError("unusable source")
            report = check(a.bin, folder)
            pages = report and report.get("layout") and report["layout"]["pages"]
            ok = bool(report and report.get("success") and pages and a.min_pages <= pages <= a.max_pages)
            over = lines_over(a.bin, folder, pages) if ok else None
            ok = ok and over is not None and 1 <= over <= a.max_over
            print(f"{pid}: {'keep' if ok else 'skip'} ({pages} pages, {over} lines over) {title[:50]}", file=sys.stderr, flush=True)
        except Exception as e:  # noqa: BLE001
            ok, pages = False, None
            print(f"{pid}: skip ({e})", file=sys.stderr)
        if ok:
            kept.append({"id": pid, "title": title, "license": lic, "pages": pages, "lines_over": over, "main": report["main"]})
            manifest_path.write_text(json.dumps(kept, indent=2) + "\n")
        else:
            subprocess.run(["rm", "-rf", str(folder)])


main()
