// Run: node --experimental-vm-modules --test tests/lyrics-ui-state.test.mjs
// Exercises the actual TypeScript modules with controlled IPC promises; no Tauri runtime.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import { create } from "zustand";

const standard = {
  music_dirs: [], menubar_lyrics: true, menubar_max_chars: 24, lyrics_scale: 100,
  online_metadata: true, menubar_only: false,
};
const snapshot = (scale) => ({ ...standard, lyrics_scale: scale });
function deferred() {
  let resolve, reject;
  const promise = new Promise((a, b) => { resolve = a; reject = b; });
  return { promise, resolve, reject };
}
async function flush() {
  for (let i = 0; i < 4; i++) await new Promise(setImmediate);
}
async function loadSource(relative, dependencies) {
  const source = await readFile(new URL(`../${relative}`, import.meta.url), "utf8");
  const code = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const context = vm.createContext({ window: { setTimeout: () => 0 }, console });
  const module = new vm.SourceTextModule(code, { context });
  await module.link(async (specifier) => {
    const values = dependencies[specifier];
    assert.ok(values, `Unexpected import: ${specifier}`);
    const dependency = new vm.SyntheticModule(Object.keys(values), function () {
      for (const [key, value] of Object.entries(values)) this.setExport(key, value);
    }, { context });
    await dependency.link(() => {});
    await dependency.evaluate();
    return dependency;
  });
  await module.evaluate();
  return module.namespace;
}
async function setup() {
  const writes = [];
  const lyricsReads = [];
  const api = {
    setSetting: (key, value) => {
      const request = { key, value, ...deferred() };
      writes.push(request);
      return request.promise;
    },
    getSettings: async () => snapshot(100),
    getLyrics: (trackId) => {
      const request = { trackId, ...deferred() };
      lyricsReads.push(request);
      return request.promise;
    },
    libraryState: async () => ({ music_dirs: [] }),
    listTracks: async () => [],
    listAlbums: async () => [{ key: "album", cover_version: "new" }],
  };
  const stores = await loadSource("src/store.ts", { zustand: { create }, "./api": { api } });
  stores.useLibrary.getState().setSettings(snapshot(100));
  return { ...stores, api, writes, lyricsReads, state: () => stores.useLibrary.getState() };
}

test("scale saves stay ordered and newest optimistic value survives earlier replies", async () => {
  const { state, writes } = await setup();
  state().saveLyricsScale(105);
  state().saveLyricsScale(160);
  state().saveLyricsScale(80);
  assert.equal(state().settings.lyrics_scale, 80);
  await flush();
  assert.deepEqual(writes.map((w) => w.value), ["105"]);
  writes[0].resolve(snapshot(105));
  await flush();
  assert.equal(state().settings.lyrics_scale, 80);
  assert.deepEqual(writes.map((w) => w.value), ["105", "160"]);
  writes[1].resolve(snapshot(160));
  await flush();
  writes[2].resolve(snapshot(80));
  await flush();
  assert.equal(state().settings.lyrics_scale, 80);
});

test("failed scale save rolls back, shows an error, and does not poison the bounded queue", async () => {
  const { state, writes, useToast } = await setup();
  state().saveLyricsScale(135);
  await flush();
  writes[0].resolve(snapshot(135));
  await flush();
  state().saveLyricsScale(145);
  await flush();
  writes[1].reject("disk full");
  await flush();
  assert.equal(state().settings.lyrics_scale, 135);
  assert.match(useToast.getState().msg, /disk full/);
  state().saveLyricsScale(999);
  await flush();
  assert.equal(writes[2].value, "300");
  writes[2].resolve(snapshot(300));
  await flush();
  assert.equal(state().settings.lyrics_scale, 300);
  state().saveLyricsScale(-5);
  await flush();
  assert.equal(writes[3].value, "80");
  writes[3].resolve(snapshot(80));
  await flush();
  assert.equal(state().settings.lyrics_scale, 80);
});

test("a stale settings read cannot undo a newer save, and the newest read wins", async () => {
  const { state, writes, api } = await setup();
  const old = deferred();
  api.getSettings = () => old.promise;
  const refresh = state().refreshSettings();
  state().saveLyricsScale(135);
  await flush();
  writes[0].resolve(snapshot(135));
  await flush();
  old.resolve(snapshot(100));
  await refresh;
  assert.equal(state().settings.lyrics_scale, 135);

  const first = deferred(), second = deferred();
  api.getSettings = () => first.promise;
  const a = state().refreshSettings();
  api.getSettings = () => second.promise;
  const b = state().refreshSettings();
  first.resolve(snapshot(80));
  await a;
  assert.equal(state().settings.lyrics_scale, 135);
  second.resolve(snapshot(160));
  await b;
  assert.equal(state().settings.lyrics_scale, 160);
});

test("lyrics event wins against an earlier same-track fetch and no-track clears lyrics", async () => {
  const { usePlayer, lyricsReads } = await setup();
  const player = () => usePlayer.getState();
  player().setTick({ track: { id: 1 } });
  const lyrics = { track_id: 1, lines: [{ t: 1, text: "new event" }], source: "test" };
  player().setLyrics(lyrics);
  lyricsReads[0].resolve(null);
  await flush();
  assert.equal(player().lyrics, lyrics);
  player().setTick({ track: { id: 2 } });
  player().setTick({ track: null });
  assert.equal(player().lyrics, null);
  lyricsReads[1].resolve({ ...lyrics, track_id: 2 });
  await flush();
  assert.equal(player().lyrics, null);
});

test("an album-view reload retains refreshed album data for its versioned hero", async () => {
  const { state, useLibrary } = await setup();
  useLibrary.setState({
    view: { kind: "album", key: "album", name: "album", artist: "artist" },
    albums: [{ key: "album", cover_version: "old" }],
  });
  await state().reload();
  assert.equal(state().albums[0].cover_version, "new");
});

test("cover URL versions are encoded after conversion, not embedded in the file path", async () => {
  const { coverUrl } = await loadSource("src/components/coverUrl.ts", {
    "@tauri-apps/api/core": { convertFileSrc: (path) => `asset://localhost/${encodeURIComponent(path)}` },
  });
  assert.equal(coverUrl(null, "v1"), null);
  assert.equal(coverUrl("/tmp/a b.jpg"), "asset://localhost/%2Ftmp%2Fa%20b.jpg");
  assert.equal(coverUrl("/tmp/a b.jpg", "1 &2"), "asset://localhost/%2Ftmp%2Fa%20b.jpg?v=1%20%262");
});
