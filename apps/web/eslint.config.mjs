import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

export default defineConfig([
  ...nextVitals,
  ...nextTs,
  globalIgnores([".next/**", ".source/**", "out/**", "build/**", "next-env.d.ts"]),
  {
    // eslint-plugin-react cannot auto-detect the React version under ESLint 10.
    settings: { react: { version: "19.2" } },
    rules: {
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },
  {
    // The design system shared with better-iam.com syncs with external systems in effects (Lenis, the theme,
    // the reduced-motion setting after hydration) and passes a stepper object that carries its ref; the
    // React Compiler rules below flag those deliberate patterns.
    files: ["components/home/**", "components/site/**", "components/application/**"],
    rules: {
      "react-hooks/set-state-in-effect": "off",
      "react-hooks/refs": "off",
    },
  },
]);
