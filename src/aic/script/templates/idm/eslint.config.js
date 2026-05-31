import js from "@eslint/js";
import prettierConfig from "eslint-config-prettier";
import prettier from "eslint-plugin-prettier";

const commonIdmRules = {
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
  "no-restricted-syntax": idmNoRestrictedSyntax,
  "prettier/prettier": "error",
  "@typescript-eslint/no-require-imports": "off",
};

const idmNoRestrictedSyntax = [
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

export default [
  js.configs.recommended,
  {
    files: ["**/*.js", "**/*.cjs"],
    languageOptions: {
      ecmaVersion: 2015,
      sourceType: "script",
      globals: {
        // IDM globals
        openidm: "readonly",
        logger: "readonly",
        request: "readonly",
        context: "readonly",
        // Node.js globals for endpoint scripts
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
    rules: commonIdmRules,
  },
  {
    files: ["endpoint/*.cjs"],
    languageOptions: {
      ecmaVersion: 2015,
      sourceType: "script",
      globals: {
        // IDM globals
        openidm: "readonly",
        logger: "readonly",
        request: "readonly",
        context: "readonly",
        // Node.js globals for endpoint scripts
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
    rules: commonIdmRules,
  },
];
