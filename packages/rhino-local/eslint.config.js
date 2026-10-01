import eslint from "@eslint/js";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: [
      "generated/**",
      "node_modules/**",
      "dist/**",
      "coverage/**",
      "corpus/**",
      "eslint.am.config.js",
      "src/bindings/rhino/**",
      "cases/**/*.cjs",
      "failures/**",
      // Declaration output of the 0.1.2 tag, vendored for test/compat.
      "test/compat/v0.1.2/**",
    ],
  },
  eslint.configs.recommended,
  ...tseslint.configs.recommended,
  {
    rules: {
      eqeqeq: "error",
      "no-unused-vars": "off",
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  }
);
