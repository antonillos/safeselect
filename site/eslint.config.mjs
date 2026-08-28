import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  // Static Pages output deliberately has no image optimizer or Next runtime.
  { files: ["app/page.tsx", "app/shared.tsx"], rules: { "@next/next/no-img-element": "off" } },
  {
    files: ["scripts/export.tsx"],
    rules: {
      "@next/next/no-head-element": "off",
      "@next/next/no-css-tags": "off",
    },
  },
  globalIgnores([
    ".next/**",
    "out/**",
    "build/**",
    ".build/**",
    "dist/**",
    "next-env.d.ts",
  ]),
]);

export default eslintConfig;
