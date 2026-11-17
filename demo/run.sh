#!/usr/bin/env bash
# End-to-end demo with invented paperwork: no private documents, no account, no API key.
#
# 1. makes a pile of fake PDFs (scripts/make_fake_docs.py),
# 2. creates a fresh box in target/demo/box,
# 3. lets the wizard check, complete, name and add every PDF of the pile,
# 4. finds a document and asks a question about the content.
#
# Needs: Rust, Python 3, Ollama running with the models of shoebox.example.yaml
# (ollama pull qwen3:8b; ollama pull bge-m3). The fake PDFs have a text layer, so no OCR runs.
#
# Usage:  demo/run.sh            (MODEL=qwen3:14b demo/run.sh to try another model)
# Result: target/demo/box/index.tsv and the renamed PDFs in target/demo/pile/
set -euo pipefail
cd "$(dirname "$0")/.."

DEMO=target/demo
SHOEBOX=target/release/shoebox

if ! curl -fsS http://localhost:11434/api/version >/dev/null 2>&1; then
    echo "error: Ollama does not answer on localhost:11434. Start it with: ollama serve" >&2
    exit 1
fi

echo "== Build"
cargo build --release --quiet

echo "== Make the fake pile"
rm -rf "$DEMO"
python3 scripts/make_fake_docs.py "$DEMO/fake" >/dev/null
# Only the pile; complete/ holds the full versions for the questions and stays out.
mkdir -p "$DEMO/pile"
cp "$DEMO"/fake/*.pdf "$DEMO/pile/"
ls "$DEMO/pile"

echo "== Create the box"
"$SHOEBOX" init "$DEMO/box"
if [[ -n "${MODEL:-}" ]]; then
    sed -i.bak "s/^model: .*/model: $MODEL/" "$DEMO/box/shoebox.yaml" && rm "$DEMO/box/shoebox.yaml.bak"
fi
export SHOEBOX_DIR="$PWD/$DEMO/box"

echo "== Wizard"
"$SHOEBOX" wizard "$DEMO/pile"

echo "== Find"
"$SHOEBOX" find "Wo ist mein Mietvertrag?"

echo "== Ask"
"$SHOEBOX" ask "Wie hoch ist die Kaution für meine Wohnung?"

echo
echo "Done: the index is $DEMO/box/index.tsv"
