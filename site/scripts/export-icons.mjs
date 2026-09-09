// Theme/raster derivatives; the reviewed SVG is the editable source of truth.
// sharp is supplied by Next through the committed npm lockfile.
import sharp from "sharp";
import { readFile, writeFile } from "node:fs/promises";

const source = await readFile(new URL("../public/icon.svg", import.meta.url));
const check = process.argv.includes("--check");

async function equivalentPng(left, right) {
  const [actual, expected] = await Promise.all([left, right].map(async bytes =>
    sharp(bytes).ensureAlpha().raw().toBuffer({ resolveWithObject: true }),
  ));
  return actual.info.width === expected.info.width
    && actual.info.height === expected.info.height
    && actual.data.equals(expected.data);
}

async function save(name, bytes) {
  const output = new URL(`../public/${name}`, import.meta.url);
  const png = name.endsWith(".png");
  let current;
  try {
    current = await readFile(output);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  let matches = current?.equals(bytes);
  if (!matches && current && png) {
    try {
      matches = await equivalentPng(current, bytes);
    } catch {
      if (check) throw new Error(`${name} differs: run npm run icons:export`);
      // Export mode repairs corrupt generated derivatives.
      matches = false;
    }
  }
  if (check) {
    if (!matches) {
      throw new Error(`${name} differs: run npm run icons:export`);
    }
  } else {
    if (matches) return;
    await writeFile(output, bytes);
  }
  console.log(`${name}: ${bytes.length} bytes${check ? " (current)" : ""}`);
}

// Light-on-dark variant for GitHub's <picture> theme switch. Geometry stays
// identical; do not apply an opaque background or change the knockout mask.
const dark = source.toString()
  .replaceAll("#152a27", "#dbe9e2")
  .replaceAll("#225b42", "#7eaf92")
  .replaceAll("#956f5d", "#b59684");
await save("icon-dark.svg", Buffer.from(dark));
for (const [name, size] of [
  ["favicon-32.png", 32],
  ["apple-touch-icon.png", 180],
  ["icon-512.png", 512],
]) {
  const png = await sharp(source, { density: 192 })
    .resize(size, size)
    .png({ palette: true, quality: 80, compressionLevel: 9, effort: 10 })
    .toBuffer();
  await save(name, png);
}
