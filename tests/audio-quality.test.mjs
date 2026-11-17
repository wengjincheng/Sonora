// Run: node --experimental-vm-modules --test tests/audio-quality.test.mjs
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import ts from "typescript";

const source = await readFile(new URL("../src/components/audioQuality.ts", import.meta.url), "utf8");
const code = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
}).outputText;
const { formatAudioQuality } = await import(`data:text/javascript;base64,${Buffer.from(code).toString("base64")}`);

test("formats source bit depth and sample rate consistently", () => {
  assert.equal(formatAudioQuality({ bit_depth: 24, sample_rate: 96000 }), "24-bit · 96 kHz");
  assert.equal(formatAudioQuality({ bit_depth: 16, sample_rate: 44100 }), "16-bit · 44.1 kHz");
  assert.equal(formatAudioQuality({ bit_depth: 24, sample_rate: 48000 }), "24-bit · 48 kHz");
});

test("omits unavailable properties instead of inventing values", () => {
  assert.equal(formatAudioQuality({ bit_depth: null, sample_rate: 44100 }), "44.1 kHz");
  assert.equal(formatAudioQuality({ bit_depth: 24, sample_rate: null }), "24-bit");
  assert.equal(formatAudioQuality({ bit_depth: null, sample_rate: null }), "");
  assert.equal(formatAudioQuality({ bit_depth: 0, sample_rate: 0 }), "");
  assert.equal(formatAudioQuality({ bit_depth: Number.NaN, sample_rate: Number.POSITIVE_INFINITY }), "");
});
