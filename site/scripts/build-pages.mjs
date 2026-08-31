import { build } from "esbuild";
import { spawnSync } from "node:child_process";
await build({
  entryPoints: ["scripts/export.tsx"],
  bundle: true,
  platform: "node",
  format: "esm",
  packages: "external",
  outfile: ".build/export.mjs",
  jsx: "automatic",
});
const result = spawnSync(process.execPath, [".build/export.mjs"], {
  stdio: "inherit",
});
if (result.status !== 0) process.exit(result.status ?? 1);
