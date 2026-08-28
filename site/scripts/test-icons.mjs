import assert from "node:assert/strict";
import { readFile, stat } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import test from "node:test";
import sharp from "sharp";

const assets = new URL("../public/", import.meta.url);
const names = ["icon.svg", "icon-dark.svg", "favicon-32.png", "apple-touch-icon.png", "icon-512.png"];

test("SVG variants are lightweight, transparent and readable in their theme", async () => {
  for (const [name, ink] of [["icon.svg", [21, 42, 39]], ["icon-dark.svg", [219, 233, 226]]]) {
    const svg = await readFile(new URL(name, assets));
    assert.ok(svg.length < 2048);
    assert.doesNotMatch(svg.toString(), /<image|<script|<foreignObject|data:/);
    const { data, info } = await sharp(svg).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
    const pixel = (x, y) => [...data.subarray((y * info.width + x) * 4, (y * info.width + x) * 4 + 4)];
    assert.equal(pixel(0, 0)[3], 0, "no opaque background");
    assert.equal(pixel(200, 330)[3], 0, "lens masks the underlying database tier");
    assert.deepEqual(pixel(256, 105), [...ink, 255]);
  }
});

test("PNG exports retain alpha and stay under the asset budget", async () => {
  for (const [name, size] of [["favicon-32.png", 32], ["apple-touch-icon.png", 180], ["icon-512.png", 512]]) {
    const file = await readFile(new URL(name, assets));
    assert.ok(file.length < 8192);
    const metadata = await sharp(file).metadata();
    assert.equal(metadata.width, size);
    assert.equal(metadata.height, size);
    assert.equal(metadata.hasAlpha, true);
    const pixel = await sharp(file).ensureAlpha().extract({ left: 0, top: 0, width: 1, height: 1 }).raw().toBuffer();
    assert.equal(pixel[3], 0);
  }
});

test("repeat export and check leave current assets unchanged", async () => {
  const snapshot = async () => Promise.all(names.map(async name => {
    const file = new URL(name, assets);
    return [(await readFile(file)).toString("base64"), (await stat(file)).mtimeMs];
  }));
  const before = await snapshot();
  for (const args of [[], ["--check"]]) {
    const run = spawnSync(process.execPath, [new URL("export-icons.mjs", import.meta.url).pathname, ...args], { encoding: "utf8" });
    assert.equal(run.status, 0, run.stderr);
    assert.deepEqual(await snapshot(), before);
  }
});
