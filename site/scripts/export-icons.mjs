// Raster derivatives only; the reviewed SVG is the editable source of truth.
// sharp is supplied by Next through the committed npm lockfile.
import sharp from "sharp";
import { readFile, writeFile } from "node:fs/promises";

const source = await readFile(new URL("../public/icon.svg", import.meta.url));
const check = process.argv.includes("--check");
for (const [name, size] of [
  ["favicon-32.png", 32],
  ["apple-touch-icon.png", 180],
  ["icon-512.png", 512],
]) {
  const output = new URL(`../public/${name}`, import.meta.url);
  const png = await sharp(source, { density: 192 })
    .resize(size, size)
    .png({ palette: true, colours: 64, compressionLevel: 9, effort: 10 })
    .toBuffer();
  if (check) {
    if (!(await readFile(output)).equals(png)) {
      throw new Error(`${name} differs: run npm run icons:export`);
    }
  } else {
    await writeFile(output, png);
  }
  console.log(`${name}: ${png.length} bytes${check ? " (current)" : ""}`);
}
