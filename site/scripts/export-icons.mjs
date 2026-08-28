// Theme/raster derivatives; the reviewed SVG is the editable source of truth.
// sharp is supplied by Next through the committed npm lockfile.
import sharp from "sharp";
import { readFile, writeFile } from "node:fs/promises";

const source = await readFile(new URL("../public/icon.svg", import.meta.url));
const check = process.argv.includes("--check");
async function save(name, bytes) {
  const output = new URL(`../public/${name}`, import.meta.url);
  if (check) {
    if (!(await readFile(output)).equals(bytes)) {
      throw new Error(`${name} differs: run npm run icons:export`);
    }
  } else {
    try {
      if ((await readFile(output)).equals(bytes)) return;
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
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
    .png({ palette: true, colours: 64, compressionLevel: 9, effort: 10 })
    .toBuffer();
  await save(name, png);
}
