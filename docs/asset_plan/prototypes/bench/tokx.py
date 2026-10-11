"""Prototype of the tok_x part of scripts/asset_bench/score.py.

usage: venv/bin/python -I tokx.py RUN_DIR [DOC ...]
tok_x = o200k(command lines) + o200k(replies) + image tokens (ceil(w/28)*ceil(h/28)) + o200k(required docs).
Prints one TSV row: calls, failed, refused, req_tok, reply_tok, img_tok, doc_tok, tok_x, wall_ms.
"""
import json, math, os, sys

import tiktoken

enc = tiktoken.get_encoding("o200k_base")
run, docs = sys.argv[1], sys.argv[2:]
rows = [json.loads(l) for l in open(os.path.join(run, "log.jsonl"))]
req = sum(len(enc.encode(r["line"])) for r in rows)
rep = sum(len(enc.encode(r["reply"])) for r in rows)
img = sum(math.ceil(r["img"][0] / 28) * math.ceil(r["img"][1] / 28) for r in rows if r.get("img"))
failed = sum(1 for r in rows if r["reply"].startswith('{"error"'))
refused = sum(1 for r in rows if r.get("refused"))
doc = sum(len(enc.encode(open(d, encoding="utf-8").read())) for d in docs)
ms = sum(r["ms"] for r in rows)
print("calls\tfailed\trefused\treq_tok\treply_tok\timg_tok\tdoc_tok\ttok_x\twall_ms")
print(f"{len(rows)}\t{failed}\t{refused}\t{req}\t{rep}\t{img}\t{doc}\t{req + rep + img + doc}\t{ms}")
