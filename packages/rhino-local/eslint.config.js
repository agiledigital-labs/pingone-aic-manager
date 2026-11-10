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
      // AM token-context probes, run byte-for-byte on AIC: ES5, AM globals,
      // and a deliberate `==` against a Java string.
      "test/fixtures/oauth2-atm/*.script.js",
      "test/fixtures/oidc-claims/*.script.js",
      "test/fixtures/social-idp/*.script.js",
      "test/fixtures/config-provider/*.script.js",
      "test/fixtures/oauth2-validate-scope/*.script.js",
      "test/fixtures/oauth2-may-act/*.script.js",
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
