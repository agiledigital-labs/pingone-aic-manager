import tsEslint from "@typescript-eslint/eslint-plugin";
import tsParser from "@typescript-eslint/parser";
import prettierConfig from "eslint-config-prettier";
import prettier from "eslint-plugin-prettier";

const amNoRestrictedSyntax = [
  "error",
  {
    selector: "VariableDeclaration[kind='let']",
    message:
      "Using 'let' is disallowed. Use 'const' instead (or var if reassignment is absolutely necessary).",
  },
  {
    selector: "ForStatement > VariableDeclaration[kind='const']",
    message:
      "Using 'const' in for loop initialization is disallowed in Rhino. Use 'var' instead.",
  },
  {
    selector: "ForInStatement > VariableDeclaration[kind='const']",
    message:
      "Using 'const' in for...in loop is disallowed in Rhino. Use 'var' instead.",
  },
  {
    selector: "ForOfStatement > VariableDeclaration[kind='const']",
    message:
      "Using 'const' in for...of loop is disallowed in Rhino. Use 'var' instead.",
  },
  {
    selector:
      "ForStatement > BlockStatement > VariableDeclaration[kind='const']",
    message:
      "Using 'const' inside a for loop body is disallowed in Rhino. Use 'var' instead.",
  },
  {
    selector:
      "ForInStatement > BlockStatement > VariableDeclaration[kind='const']",
    message:
      "Using 'const' inside a for...in loop body is disallowed in Rhino. Use 'var' instead.",
  },
  {
    selector:
      "ForOfStatement > BlockStatement > VariableDeclaration[kind='const']",
    message:
      "Using 'const' inside a for...of loop body is disallowed in Rhino. Use 'var' instead.",
  },
  {
    selector: "ObjectExpression > Property[method=false][shorthand=true]",
    message:
      "Using object shorthand is disallowed in Rhino. Use full property syntax instead.",
  },
  {
    selector: "VariableDeclarator > ObjectPattern",
    message: "Using object destructuring is disallowed in Rhino.",
  },
];

const commonAmRules = {
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
  "no-inner-declarations": "off",
  "no-plusplus": [
    "warn",
    {
      allowForLoopAfterthoughts: true,
    },
  ],
  "no-alert": "error",
  "no-template-curly-in-string": "error",
  "prefer-template": "warn",
  "no-implicit-coercion": "warn",
  curly: "error",
  "object-shorthand": ["error", "never"],
  "require-unicode-regexp": "off",
  "no-undef": "off",
  "no-restricted-syntax": amNoRestrictedSyntax,
};

export default [
  // Configuration for JavaScript/CommonJS files (actual scripts)
  {
    files: ["**/*.cjs"],
    languageOptions: {
      ecmaVersion: 2015,
      sourceType: "script",
      globals: {
        // ForgeRock/AM script globals
        logger: "readonly",
        httpClient: "readonly",
        openidm: "readonly",
        requestHeaders: "readonly",
        request: "readonly",
        context: "readonly",
        username: "readonly",
        realm: "readonly",
        sharedState: "readonly",
        transientState: "readonly",
        idRepository: "readonly",
        systemEnv: "readonly",
        outcome: "readonly",
        nodeState: "readonly",
        scriptName: "readonly",
        callbacks: "readonly",
        callbacksBuilder: "readonly",
        action: "readonly",
        // Node.js globals
        console: "readonly",
        process: "readonly",
        Buffer: "readonly",
        __dirname: "readonly",
        __filename: "readonly",
        module: "readonly",
        require: "readonly",
        exports: "readonly",
        global: "readonly",
        setTimeout: "readonly",
        clearTimeout: "readonly",
        setInterval: "readonly",
        clearInterval: "readonly",
      },
    },
    plugins: {
      prettier: prettier,
    },
    rules: {
      ...prettierConfig.rules,
      "no-inner-declarations": "off",
      "no-plusplus": [
        "warn",
        {
          allowForLoopAfterthoughts: true,
        },
      ],
      "no-alert": "error",
      "no-template-curly-in-string": "error",
      "prefer-template": "warn",
      "no-implicit-coercion": "warn",
      curly: "error",
      "object-shorthand": ["error", "never"],
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
      "no-restricted-syntax": amNoRestrictedSyntax,
      "prettier/prettier": "error",
    },
  },
  // Configuration for source scripts (stricter rules)
  {
    files: ["**/src/**/*.cjs"],
    languageOptions: {
      ecmaVersion: 2015,
      sourceType: "script",
    },
    rules: {
      "no-restricted-syntax": [
        ...amNoRestrictedSyntax,
        {
          selector: "Program > VariableDeclaration[kind='const']",
          message:
            "Using 'const' at the root level is disallowed in Rhino. Use 'var' instead.",
        },
      ],
    },
  },
  // Configuration for TypeScript definition files
  {
    files: ["**/*.d.ts"],
    languageOptions: {
      parser: tsParser,
      ecmaVersion: 2015,
      sourceType: "module",
    },
    plugins: {
      "@typescript-eslint": tsEslint,
      prettier: prettier,
    },
    rules: commonAmRules,
  },
];
