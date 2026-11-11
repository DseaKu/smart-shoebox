#!/usr/bin/env python3
"""Measure how well shoebox answers questions about the fake documents.

Usage:
    python3 scripts/eval_rag.py [--mode find|search|ask] [--method vector|keyword|hybrid]
                                [--fresh] [--model NAME] [--kind KIND]

1. Makes the fake documents if needed (scripts/make_fake_docs.py).
2. Fills an evaluation box in target/eval-box/ (ignored by git) with the
   documents in testdata/fake/eval_docs.txt, using `shoebox -y add`. The box is
   kept, so later runs skip this step; --fresh starts again.
3. Checks that each answer is in the box's saved page text (else the question
   cannot be answered at all, whatever the search does).
4. Asks every question in testdata/fake/questions.tsv and scores it.

Modes:
  find    (the baseline) searches the index rows, not the text inside the
          documents. Scored per document: is the expected document among the
          results, and how many other documents came back. For questions of kind
          `none`, "correct" means no document was returned.
  search  `shoebox search`: the chunks closest to the question (ROADMAP 7.3/7.4).
          Scored per page: is an expected page among the top 1, 3, 5 chunks
          (recall@k)? Questions of kind `none` are left out: a search always
          returns its best chunks; saying "not found" is the answer's job (7.5).
  ask     `shoebox ask`: the answer and its checked sources (ROADMAP 7.5).
          answer  the expected answer is in it (same words, same number or
                  date in another format, or the same reference number);
          source  a source is on an expected page;
          wrong   an answer was given, but not the expected one;
          missed  "not found" although the documents have the answer.
          For kind `none`, correct means "not found".

Needs a running Ollama. Results go to target/eval-results/.
"""

import argparse
import collections
import datetime
import hashlib
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FAKE = ROOT / "testdata" / "fake"
BOX = ROOT / "target" / "eval-box"
RESULTS = ROOT / "target" / "eval-results"
SHOEBOX = ROOT / "target" / "debug" / "shoebox"
STORE_PATH = re.compile(r"store/([0-9a-f]{64})\.pdf")
SEARCH_LINE = re.compile(r"^\s*\d+\.\s+(-?[\d.]+)\s+.*? p(\d+)(?: part \d+)?\s+\(([0-9a-f]{12})\)$")
TOP_K = (1, 3, 5)


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def norm(text):
    return " ".join(text.lower().split())


def run(args, **kwargs):
    return subprocess.run(args, capture_output=True, text=True, **kwargs)


def shoebox(*args):
    result = run([str(SHOEBOX), *args], cwd=BOX)
    if result.returncode != 0:
        raise RuntimeError(f"shoebox {' '.join(args)} failed:\n{result.stderr}")
    return result.stdout


def read_questions(kind):
    lines = (FAKE / "questions.tsv").read_text(encoding="utf-8").splitlines()
    header = lines[0].split("\t")
    questions = [dict(zip(header, line.split("\t"))) for line in lines[1:] if line]
    for q in questions:
        q["pages"] = [int(n) for n in q["pages"].split(",") if n]
    return [q for q in questions if not kind or q["kind"] == kind]


def prepare_box(fresh, model):
    if fresh and BOX.exists():
        shutil.rmtree(BOX)
    if not (BOX / "shoebox.yaml").is_file():
        result = run([str(SHOEBOX), "init", str(BOX)])
        if result.returncode != 0:
            sys.exit(result.stderr)
    if model:
        config = BOX / "shoebox.yaml"
        text = re.sub(r"(?m)^model:.*$", f"model: {model}", config.read_text())
        config.write_text(text)
    index = (BOX / "index.tsv").read_text(encoding="utf-8")
    files = {}
    for name in (FAKE / "eval_docs.txt").read_text().split():
        path = FAKE / name
        digest = sha256(path)
        files[digest] = name
        if digest not in index:
            print(f"adding {name} …", flush=True)
            shoebox("-y", "add", str(path))
    return files


def check_page_text(questions, files):
    """The answer must be in the saved page text of the expected page."""
    by_name = {name: digest for digest, name in files.items()}
    problems = []
    for q in questions:
        if q["kind"] == "none":
            continue
        pages_file = BOX / "store" / f"{by_name[q['file']]}.pages.json"
        pages = json.loads(pages_file.read_text(encoding="utf-8"))["pages"]
        texts = [norm(pages[n - 1]) for n in q["pages"]]
        if not any(norm(alt) in t for alt in q["answer"].split(" | ") for t in texts):
            problems.append(f"{q['id']}: {q['answer']!r} not in the page text of {q['file']}")
    return problems


def eval_find(questions, files):
    rows = []
    for q in questions:
        out = shoebox("find", q["question"])
        found = [files.get(h, f"unknown:{h[:8]}") for h in STORE_PATH.findall(out)]
        if q["kind"] == "none":
            correct = not found
            extra = len(found)
        else:
            correct = q["file"] in found
            extra = len([f for f in found if f != q["file"]])
        rows.append({**q, "found": found, "correct": correct, "extra": extra})
        mark = "ok  " if correct else "MISS"
        print(f"{mark} {q['id']} {q['kind']:8} {q['question']}")
        if not correct or extra:
            print(f"       expected: {q['file'] or '(nothing)'}; got: {', '.join(found) or '(nothing)'}")
    return rows


def eval_search(questions, files, method):
    by_prefix = {digest[:12]: name for digest, name in files.items()}
    rows = []
    for q in questions:
        if q["kind"] == "none":
            continue
        out = shoebox("search", "--method", method, "--top", str(max(TOP_K)), q["question"])
        found = []
        for line in out.splitlines():
            m = SEARCH_LINE.match(line)
            if m:
                found.append((by_prefix.get(m.group(3), "?"), int(m.group(2)), float(m.group(1))))
        rank = next((i + 1 for i, (f, page, _) in enumerate(found)
                     if f == q["file"] and page in q["pages"]), None)
        doc_rank = next((i + 1 for i, (f, _, _) in enumerate(found) if f == q["file"]), None)
        rows.append({**q, "found": [f"{f}:p{page}" for f, page, _ in found],
                     "rank": rank, "doc_rank": doc_rank})
        mark = f"@{rank}" if rank else "MISS"
        print(f"{mark:4} {q['id']} {q['kind']:8} {q['question']}")
        if rank != 1:
            print(f"       expected: {q['file']} p{','.join(map(str, q['pages']))}; "
                  f"got: {', '.join(rows[-1]['found'][:3])}")
    return rows


def report_search(rows, label):
    def recall(group, k, key="rank"):
        return sum(1 for r in group if r[key] and r[key] <= k)

    by_kind = collections.defaultdict(list)
    for row in rows:
        by_kind[row["kind"]].append(row)
    head = "  ".join(f"p@{k}" for k in TOP_K)
    print(f"\nsearch {label}: correct page among the top k chunks (doc = correct document)")
    print(f"  {'kind':8} {'n':>2}  {head}  doc@1")
    for kind, group in sorted(by_kind.items()) + [("all", rows)]:
        cells = "  ".join(f"{recall(group, k):3}" for k in TOP_K)
        print(f"  {kind:8} {len(group):2}  {cells}  {recall(group, 1, 'doc_rank'):5}")

    RESULTS.mkdir(parents=True, exist_ok=True)
    stamp = datetime.datetime.now().strftime("%Y-%m-%d_%H%M%S")
    out = RESULTS / f"search_{label.split()[0]}_{stamp}.tsv"
    lines = ["id\tkind\trank\tdoc_rank\texpected\tfound\tquestion"]
    for r in rows:
        lines.append("\t".join([r["id"], r["kind"], str(r["rank"] or ""), str(r["doc_rank"] or ""),
                                f"{r['file']}:p{','.join(map(str, r['pages']))}",
                                ",".join(r["found"]), r["question"]]))
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"details: {out}")


SOURCE_LINE = re.compile(r"^  - .* p(\d+) \(([0-9a-f]{12})\): ")
NOT_FOUND = "Not found in your documents."
DATE = re.compile(r"^(\d{2})\.(\d{2})\.(\d{4})$")
NUMBER = re.compile(r"\d[\d.,]*\d|\d")


def german_number(token):
    """2.340,00 → 2340.0; 164,80 → 164.8; 2.340 → 2340; 01.04.2024 → None."""
    if "," in token:
        token = token.replace(".", "").replace(",", ".")
    elif token.count(".") == 1 and len(token.split(".")[1]) == 3:
        token = token.replace(".", "")
    elif "." in token:
        return None
    try:
        return float(token)
    except ValueError:
        return None


def answer_ok(expected, answer):
    a = norm(answer)
    a_alnum = re.sub(r"[^0-9a-zäöüß]", "", a)
    for alt in expected.split(" | "):
        e = norm(alt)
        if e in a:
            return True
        e_alnum = re.sub(r"[^0-9a-zäöüß]", "", e)
        if re.search(r"\d", e) and len(e_alnum) >= 6 and e_alnum in a_alnum:
            return True  # reference numbers with other separators
        date = DATE.match(alt)
        if date:
            d, m, y = date.groups()
            if any(v in answer for v in (f"{int(d)}.{int(m)}.{y}", f"{y}-{m}-{d}")):
                return True
            continue
        wanted = [german_number(t) for t in NUMBER.findall(alt)]
        if len(wanted) == 1 and wanted[0] is not None:
            got = {german_number(t) for t in NUMBER.findall(answer)}
            if wanted[0] in got:
                return True
    return False


def eval_ask(questions, files):
    by_prefix = {digest[:12]: name for digest, name in files.items()}
    rows = []
    for q in questions:
        started = datetime.datetime.now()
        result = run([str(SHOEBOX), "ask", q["question"]], cwd=BOX)
        seconds = (datetime.datetime.now() - started).total_seconds()
        if result.returncode != 0:
            answer, sources, verdict = f"error: {result.stderr.strip()}", [], "error"
        else:
            lines = result.stdout.splitlines()
            answer = lines[0] if lines else ""
            sources = []
            for line in lines:
                m = SOURCE_LINE.match(line)
                if m:
                    sources.append((by_prefix.get(m.group(2), "?"), int(m.group(1))))
            not_found = answer == NOT_FOUND
            if q["kind"] == "none":
                verdict = "correct" if not_found else "wrong"
            elif not_found:
                verdict = "missed"
            else:
                verdict = "correct" if answer_ok(q["answer"], answer) else "wrong"
        source_ok = any(f == q["file"] and p in q["pages"] for f, p in sources)
        rows.append({**q, "answer_text": answer, "verdict": verdict, "source_ok": source_ok,
                     "sources": [f"{f}:p{p}" for f, p in sources], "seconds": seconds})
        mark = {"correct": "ok  ", "wrong": "WRONG", "missed": "MISS", "error": "ERR "}[verdict]
        print(f"{mark:5} {q['id']} {q['kind']:8} {q['question']}  ({seconds:.0f} s)")
        if verdict != "correct":
            unchecked = [l.strip() for l in result.stderr.splitlines() if "unchecked" in l or l.strip().startswith("(")]
            print(f"       expected: {q['answer'] or '(not found)'}; got: {answer}")
            for line in unchecked:
                print(f"       {line}")
    return rows


def report_ask(rows, label):
    by_kind = collections.defaultdict(list)
    for row in rows:
        by_kind[row["kind"]].append(row)
    print(f"\nask {label}")
    print(f"  {'kind':8} {'n':>2}  correct  source  wrong  missed")
    for kind, group in sorted(by_kind.items()) + [("all", rows)]:
        count = lambda v: sum(1 for r in group if r["verdict"] == v)
        sources = sum(1 for r in group if r["source_ok"])
        print(f"  {kind:8} {len(group):2}  {count('correct'):7}  {sources:6}  {count('wrong'):5}  {count('missed'):6}")
    seconds = [r["seconds"] for r in rows]
    print(f"  {sum(seconds) / len(seconds):.1f} s per question on average")

    RESULTS.mkdir(parents=True, exist_ok=True)
    stamp = datetime.datetime.now().strftime("%Y-%m-%d_%H%M%S")
    out = RESULTS / f"ask_{stamp}.tsv"
    lines = ["id\tkind\tverdict\tsource_ok\texpected\tanswer\tsources\tquestion"]
    for r in rows:
        lines.append("\t".join([r["id"], r["kind"], r["verdict"], str(r["source_ok"]), r["answer"],
                                r["answer_text"], ",".join(r["sources"]), r["question"]]))
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"details: {out}")


def report(rows, model):
    by_kind = collections.defaultdict(list)
    for row in rows:
        by_kind[row["kind"]].append(row)
    print(f"\nfind with {model}: {sum(r['correct'] for r in rows)} of {len(rows)} correct")
    for kind, group in sorted(by_kind.items()):
        correct = sum(r["correct"] for r in group)
        extra = sum(r["extra"] for r in group)
        print(f"  {kind:8} {correct:2} of {len(group):2} correct, {extra} extra documents")

    RESULTS.mkdir(parents=True, exist_ok=True)
    stamp = datetime.datetime.now().strftime("%Y-%m-%d_%H%M%S")
    out = RESULTS / f"find_{stamp}.tsv"
    lines = ["id\tkind\tcorrect\textra\texpected\tfound\tquestion"]
    for r in rows:
        lines.append("\t".join([r["id"], r["kind"], str(r["correct"]), str(r["extra"]),
                                r["file"], ",".join(r["found"]), r["question"]]))
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"details: {out}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--mode", choices=["find", "search", "ask"], default="find",
                        help="what to measure (default: find)")
    parser.add_argument("--method", choices=["vector", "keyword", "hybrid"], default="hybrid",
                        help="search mode: how to rank the chunks (default: hybrid)")
    parser.add_argument("--fresh", action="store_true", help="start with an empty box")
    parser.add_argument("--model", help="Ollama model for the box (default: as in its shoebox.yaml)")
    parser.add_argument("--kind", help="only questions of this kind (fact, synonym, number, …)")
    args = parser.parse_args()

    if run(["cargo", "build", "-q"], cwd=ROOT).returncode != 0:
        sys.exit("cargo build failed")
    if not (FAKE / "questions.tsv").is_file() or not (FAKE / "complete").is_dir():
        run([sys.executable, "-I", str(ROOT / "scripts" / "make_fake_docs.py")], check=True)

    files = prepare_box(args.fresh, args.model)
    config = (BOX / "shoebox.yaml").read_text()
    model = re.search(r"(?m)^model:\s*(\S+)", config).group(1)
    embed = re.search(r"(?m)^embed_model:\s*(\S+)", config)
    embed_model = embed.group(1) if embed else "bge-m3 (default)"
    questions = read_questions(args.kind)
    problems = check_page_text(questions, files)
    if problems:
        print("The page text misses some answers (fix the documents or the extraction first):")
        print("\n".join("  " + p for p in problems))
        sys.exit(1)
    print(f"page text: all {len(questions)} answers are in the saved text\n")
    if args.mode == "find":
        report(eval_find(questions, files), model)
    elif args.mode == "ask":
        report_ask(eval_ask(questions, files), f"with {model}, search hybrid ({embed_model})")
    else:
        label = f"{args.method} ({embed_model})"
        report_search(eval_search(questions, files, args.method), label)


if __name__ == "__main__":
    main()
