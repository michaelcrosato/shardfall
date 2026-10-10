import readline from 'node:readline';
import { handle } from './engine.mjs';

// Stdout is JSONL only. A failed request does not end the persistent worker.
const lines = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of lines) {
  if (!line.trim()) continue;
  let reply;
  try {
    if (line.length > 8 * 1024 * 1024) throw new Error('Compiler request exceeds 8 MiB');
    reply = handle(JSON.parse(line));
  } catch (error) {
    reply = { id: null, ok: false, error: error.message ?? String(error), issues: [] };
  }
  if (!process.stdout.write(`${JSON.stringify(reply)}\n`)) {
    await new Promise((resolve) => process.stdout.once('drain', resolve));
  }
}
