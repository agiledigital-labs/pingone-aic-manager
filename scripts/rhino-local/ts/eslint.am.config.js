// Rhino 1.7.14 restrictions for the generated mock surface.
//
// Sourced from src/scripts/templates/am/eslint.config.js — the project's AM
// script lint. That config cannot be imported here: it pulls prettier and a
// managed query-filter JSON via import.meta.url, and its `files` globs target
// a scaffolded workspace (`*/decision-node/**/*.cjs`). The selectors, missing
// globals, and custom rule implementations are copied; a lockstep test fails
// if the AM config grows a restriction this file does not have.
//
// Applied as a scripted-decision global-scope script (top-level `const` is
// banned), because the generated mock is evaluated in that scope.

const rhinoParseErrors = [
  {
    selector: "VariableDeclaration[kind='let']",
    message:
      "'let' is a parse error on Rhino 1.7.14 ('missing ; before statement'). Use 'var' (or 'const' inside a function).",
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
    selector: "ForOfStatement",
    message:
      "'for...of' is a parse error on Rhino 1.7.14 ('missing ; after for-loop initializer') — the whole statement, not just its 'const' form. Use an indexed 'for' loop over arrays; 'for...in' works but yields keys, not values.",
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

const rhinoMissingGlobals = [
  ["Map", "Use a plain object keyed by strings: `o[key] = value`."],
  ["WeakMap", "Use a plain object keyed by strings: `o[key] = value`."],
  [
    "Set",
    "Use a plain object as a seen-set: `if (!seen[item]) { seen[item] = true; }`. " +
      "`new java.util.HashSet()` also works if you need Java collection semantics.",
  ],
  [
    "WeakSet",
    "Use a plain object as a seen-set: `if (!seen[item]) { seen[item] = true; }`.",
  ],
  [
    "Symbol",
    "There is no substitute. Its absence is also why for...of and other iteration protocols do not work.",
  ],
  [
    "Promise",
    "There is no substitute, and none is needed: AM scripts are synchronous. " +
      "`httpClient.send(...).get()` already blocks for the response.",
  ],
  ["Proxy", "There is no substitute; restructure to plain property access."],
  [
    "Reflect",
    "There is no substitute; use direct property access or `Object.keys`.",
  ],
].map(([name, hint]) => ({
  name,
  message: `${name} is absent from AM's Rhino at runtime (ReferenceError on use) — see docs/api/12-script-bindings-matrix.md. ${hint}`,
}));

const noDupConstFunctionScoped = {
  meta: {
    type: "problem",
    docs: {
      description:
        "disallow re-declaring the same const name within one function (Rhino function-scoped const)",
    },
    schema: [],
  },
  create(context) {
    const stack = [];
    const push = function () {
      stack.push(new Set());
    };
    const pop = function () {
      stack.pop();
    };
    return {
      Program: push,
      "Program:exit": pop,
      FunctionDeclaration: push,
      "FunctionDeclaration:exit": pop,
      FunctionExpression: push,
      "FunctionExpression:exit": pop,
      ArrowFunctionExpression: push,
      "ArrowFunctionExpression:exit": pop,
      VariableDeclaration: function (node) {
        if (node.kind !== "const") return;
        const seen = stack[stack.length - 1];
        if (!seen) return;
        for (const decl of node.declarations) {
          if (!decl.id || decl.id.type !== "Identifier") continue;
          if (seen.has(decl.id.name)) {
            context.report({
              node: decl.id,
              message:
                "'{{name}}' const is re-declared in this function. Rhino 1.7.14 scopes const to the whole function, so the name must be unique per function (even across separate blocks). Rename one, or use 'var'.",
              data: { name: decl.id.name },
            });
          } else {
            seen.add(decl.id.name);
          }
        }
      },
    };
  },
};

const LOOP_NODE_TYPES = new Set([
  "ForStatement",
  "ForInStatement",
  "ForOfStatement",
  "WhileStatement",
  "DoWhileStatement",
]);

function isFunctionNode(node) {
  return (
    node &&
    (node.type === "FunctionDeclaration" ||
      node.type === "FunctionExpression" ||
      node.type === "ArrowFunctionExpression")
  );
}

function isLoopInitializer(node) {
  const parent = node.parent;
  return (
    parent &&
    ((parent.type === "ForStatement" && parent.init === node) ||
      (parent.type === "ForInStatement" && parent.left === node) ||
      (parent.type === "ForOfStatement" && parent.left === node))
  );
}

function isInsideLoopBody(node) {
  for (let current = node.parent; current; current = current.parent) {
    if (isFunctionNode(current)) return false;
    if (LOOP_NODE_TYPES.has(current.type)) return true;
  }
  return false;
}

const noConstInLoopBody = {
  meta: {
    type: "problem",
    docs: {
      description:
        "disallow const inside loop bodies where Rhino reads it back as undefined",
    },
    schema: [],
  },
  create(context) {
    return {
      VariableDeclaration(node) {
        if (node.kind !== "const") return;
        if (isLoopInitializer(node)) return;
        if (!isInsideLoopBody(node)) return;

        context.report({
          node,
          message:
            "'const' inside a for/for-in/for-of/while/do-while loop body parses but reads back as undefined on Rhino 1.7.14, including when nested in an if/block. Use 'var'.",
        });
      },
    };
  },
};

const decisionConstScopeBugs = [
  {
    selector: "Program > VariableDeclaration[kind='const']",
    message:
      "Top-level 'const' parses but reads back as undefined in a scripted-decision script on Rhino 1.7.14. Use 'var' at the top level.",
  },
];

const rhinoPlugin = {
  rules: {
    "no-dup-const": noDupConstFunctionScoped,
    "no-const-in-loop-body": noConstInLoopBody,
  },
};

export default [
  {
    files: ["generated/**/*.cjs", "src/bindings/**/*.cjs", "cases/**/*.cjs"],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "script",
    },
    plugins: {
      rhino: rhinoPlugin,
    },
    rules: {
      curly: "error",
      "no-undef": "off",
      "no-restricted-globals": ["error", ...rhinoMissingGlobals],
      "no-restricted-syntax": [
        "error",
        ...rhinoParseErrors,
        ...decisionConstScopeBugs,
      ],
      "rhino/no-dup-const": "error",
      "rhino/no-const-in-loop-body": "error",
    },
  },
];
