// Reads a corpus of messages written by the Rust encoder and runs them through the generated
// TypeScript codec: decode, dump every field (the canonical text of the generated support
// module), re-encode. The Rust test compares the answers with its own dumps and the original bytes.
//
//   node cross.ts <schema.ts> <schema.support.ts> <Message> <corpus.bin> <out.txt>
//
// Corpus: records of [u32 length][bytes]; a length of 0xFFFFFFFF starts a new stream (fresh tables).
// Answer: three lines per message: the dump, the dump of the tables, the hex of the re-encoded bytes.
import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const [schemaPath, supportPath, message, corpusPath, outPath] = process.argv.slice(2);
if (!schemaPath || !supportPath || !message || !corpusPath || !outPath) throw new Error("usage: cross.ts schema support message corpus out");

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const schema = (await import(pathToFileURL(schemaPath).href)) as Record<string, any>;
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const support = (await import(pathToFileURL(supportPath).href)) as Record<string, any>;
const decode = schema[`decode${message}`];
const encode = schema[`encode${message}`];
const dump = support[`dump${message}`];
if (!decode || !encode || !dump) throw new Error(`no decode/encode/dump for ${message}`);

const buf = readFileSync(corpusPath);
const lines: string[] = [];
let tables = schema.newTables();
let pos = 0;
while (pos < buf.length) {
  const len = buf.readUInt32LE(pos);
  pos += 4;
  if (len === 0xffffffff) {
    tables = schema.newTables();
    continue;
  }
  const bytes = new Uint8Array(buf.subarray(pos, pos + len));
  pos += len;
  const lens: number[] = schema.tableLens(tables);
  const before = tables.generation;
  const msg = decode(bytes, tables);
  const reset = before !== tables.generation;
  const state = { generation: tables.generation, tables, sent: reset ? lens.map(() => 0) : lens };
  const again: Uint8Array = encode(msg, state);
  lines.push(dump(msg, tables), support.dumpTables(tables), Buffer.from(again).toString("hex"));
}
writeFileSync(outPath, lines.join("\n") + "\n");
