import tsEslint from "@typescript-eslint/eslint-plugin";
import tsParser from "@typescript-eslint/parser";
import prettierConfig from "eslint-config-prettier";
import prettier from "eslint-plugin-prettier";

// AM scripts run on Mozilla Rhino 1.7.14 (both the legacy and next-generation
// engines). The restrictions below are RUNTIME-VERIFIED against the sandbox —
// see docs/api/12-script-bindings-matrix.md and scripts/rhino-script-tester/.
// `let`, object shorthand, object/array destructuring, default parameters, and
// `const` in any loop position are parse errors; top-level / loop-body `const`
// parses but silently reads back `undefined`. `const` inside a function works
// and is allowed. Arrow functions, template literals, and ES2015 Array/String/
// Object methods all work and are NOT restricted.
//
// ecmaVersion is set high enough for the parser to PRODUCE these nodes so they
// can be flagged — parsing support is not runtime permission.
const rhinoSyntaxRestrictions = [
  "error",
  {
    selector: "VariableDeclaration[kind='let']",
    message:
      "'let' is a parse error on Rhino 1.7.14 ('missing ; before statement'). Use 'var' (or 'const' inside a function).",
  },
  {
    selector: "Program > VariableDeclaration[kind='const']",
    message:
      "Top-level 'const' parses but reads back as undefined on Rhino 1.7.14. Use 'var' at the top level.",
  },
  {
    selector:
      ":matches(ForStatement, ForInStatement, ForOfStatement) > BlockStatement > VariableDeclaration[kind='const']",
    message:
      "'const' inside a loop body parses but reads back as undefined on Rhino 1.7.14. Use 'var'.",
  },
  {
    selector: "ForStatement > VariableDeclaration[kind='const']",
    message:
      "'const' in a for-loop initializer is a parse error on Rhino 1.7.14. Use 'var'.",
  },
  {
    selector: "ForInStatement > VariableDeclaration[kind='const']",
    message:
      "'const' in a for...in initializer is a parse error on Rhino 1.7.14. Use 'var'.",
  },
  {
    selector: "ForOfStatement > VariableDeclaration[kind='const']",
    message:
      "'const' in a for...of initializer is a parse error on Rhino 1.7.14. Use 'var'.",
  },
  {
    selector: "ObjectExpression > Property[shorthand=true]",
    message:
      "Object shorthand ({ a }) is a parse error on Rhino 1.7.14 ('missing : after property id'). Use full { a: a } syntax.",
  },
  {
    selector: "ObjectPattern",
    message:
      "Object destructuring is a parse error on Rhino 1.7.14. Assign properties explicitly.",
  },
  {
    selector: "ArrayPattern",
    message:
      "Array destructuring is a parse error on Rhino 1.7.14. Index elements explicitly.",
  },
  {
    selector: "AssignmentPattern",
    message:
      "Default parameter values are a parse error on Rhino 1.7.14. Assign defaults inside the function body.",
  },
];

// Quality rules applied to every script. `no-undef` is OFF: TypeScript (via the
// layered types/ .d.ts files and checkJs) is the source of truth for undefined
// bindings, so we don't duplicate it here. The per-family `globals` blocks below
// document the binding set each family sees and keep tooling consistent.
const scriptRules = {
  ...prettierConfig.rules,
  "no-inner-declarations": "off",
  "no-plusplus": ["warn", { allowForLoopAfterthoughts: true }],
  "no-alert": "error",
  "no-template-curly-in-string": "error",
  "prefer-template": "warn",
  "no-implicit-coercion": "warn",
  curly: "error",
  "no-unused-vars": [
    "warn",
    {
      argsIgnorePattern: "^_",
      varsIgnorePattern: "^_",
      caughtErrorsIgnorePattern: "^_",
    },
  ],
  "require-unicode-regexp": "off",
  "no-undef": "off",
  "no-restricted-syntax": rhinoSyntaxRestrictions,
  "prettier/prettier": "error",
};

// Common bindings present in all next-generation AM scripts (verified).
const commonGlobals = {
  logger: "readonly",
  httpClient: "readonly",
  openidm: "readonly",
  utils: "readonly",
  systemEnv: "readonly",
  realm: "readonly",
  scriptName: "readonly",
};

export default [
  // All AM scripts: Rhino syntax rules + common next-gen bindings.
  {
    files: ["*/**/*.cjs"],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "script",
      globals: { ...commonGlobals },
    },
    plugins: { prettier },
    rules: scriptRules,
  },
  // Next-generation scripted decision: decision bindings + library require().
  {
    files: ["*/decision-node/**/*.cjs"],
    languageOptions: {
      globals: {
        nodeState: "readonly",
        callbacks: "readonly",
        callbacksBuilder: "readonly",
        requestHeaders: "readonly",
        requestParameters: "readonly",
        requestCookies: "readonly",
        idRepository: "readonly",
        action: "readonly",
        outcome: "writable",
        existingSession: "readonly",
        resumedFromSuspend: "readonly",
        secrets: "readonly",
        require: "readonly",
        module: "readonly",
        exports: "writable",
      },
    },
  },
  // Legacy scripted decision: legacy state + Java interop, NO library require().
  {
    files: ["*/decision-node-legacy/**/*.cjs"],
    languageOptions: {
      globals: {
        nodeState: "readonly",
        callbacks: "readonly",
        requestHeaders: "readonly",
        requestParameters: "readonly",
        idRepository: "readonly",
        action: "readonly",
        outcome: "writable",
        existingSession: "readonly",
        sharedState: "readonly",
        transientState: "readonly",
        JavaImporter: "readonly",
      },
    },
  },
  // Library scripts: CommonJS module mechanics (next-gen only).
  {
    files: ["*/lib/**/*.cjs"],
    languageOptions: {
      globals: {
        require: "readonly",
        module: "readonly",
        exports: "writable",
      },
    },
  },
  // OIDC claims (legacy binding set).
  {
    files: ["*/oidc-claims/**/*.cjs"],
    languageOptions: {
      globals: {
        scopes: "readonly",
        claims: "readonly",
        requestedClaims: "readonly",
        claimObjects: "readonly",
        requestedTypedClaims: "readonly",
        claimsLocales: "readonly",
        requestProperties: "readonly",
        clientProperties: "readonly",
        identity: "readonly",
        session: "readonly",
        JavaImporter: "readonly",
        org: "readonly",
        java: "readonly",
      },
    },
  },
  // TypeScript definition files (the managed types/ set).
  {
    files: ["**/*.d.ts"],
    languageOptions: {
      parser: tsParser,
      ecmaVersion: 2022,
      sourceType: "module",
    },
    plugins: { "@typescript-eslint": tsEslint, prettier },
    rules: {
      ...tsEslint.configs.recommended.rules,
      ...prettierConfig.rules,
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          argsIgnorePattern: "^_",
          varsIgnorePattern: "^_",
          caughtErrorsIgnorePattern: "^_",
        },
      ],
      "@typescript-eslint/no-explicit-any": "warn",
      "@typescript-eslint/explicit-function-return-type": "off",
      "@typescript-eslint/explicit-module-boundary-types": "off",
      "@typescript-eslint/no-require-imports": "off",
      "prettier/prettier": "error",
    },
  },
];
