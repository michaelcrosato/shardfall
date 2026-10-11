"""tok_cum: sum over agent turns (wrapper invocations) of the context before that turn: docs + earlier traffic
(requests + replies + images), excluding the runner's own system prompt. Prototype of score.py's tok_cum."""
import json, math, os, sys
import tiktoken
enc = tiktoken.get_encoding("o200k_base")
run, docs = sys.argv[1], sys.argv[2:]
rows = [json.loads(l) for l in open(os.path.join(run, "log.jsonl"))]
doc = sum(len(enc.encode(open(d, encoding="utf-8").read())) for d in docs)
per = {}
for r in rows:
    t = len(enc.encode(r["line"])) + len(enc.encode(r["reply"]))
    if r.get("img"): t += math.ceil(r["img"][0] / 28) * math.ceil(r["img"][1] / 28)
    per[r["inv"]] = per.get(r["inv"], 0) + t
ctx, cum = doc, 0
for k in sorted(per):
    cum += ctx          # the model reads the context, then issues this invocation
    ctx += per[k]
cum += ctx              # final turn reads everything and reports
print(f"turns={len(per)}\tdoc={doc}\ttok_x={ctx}\ttok_cum={cum}")
