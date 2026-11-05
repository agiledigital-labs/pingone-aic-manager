// The one CREST _queryFilter grammar and evaluator for both lanes: the
// script tester's openidm.query (runtime.cjs, as text in Rhino and node:vm)
// and the IDM emulator (src/idm/filter.ts, as CommonJS). Neither lane may
// grow its own; a lane supplies only its string rule (see `evaluate`).
//
// parse(input) answers one of three ways, and the split is the point (E19):
//   { ast }                        a filter this grammar reads, and AIC does.
//   { error: "parse", reason }     AIC rejects it with its parse error. Only
//                                  for inputs measured to be rejected, or
//                                  rejected by every grammar CREST could have.
//   { error: "unsupported", reason } anything else this grammar cannot read.
//                                  The lane raises a harness fault: a
//                                  catchable error here would be a false pass
//                                  wherever AIC parses the input after all.
//
// Measured (docs/api/10, docs/api/12): `name eq` (missing value, script text);
// the word form `not (...)`; `eq null`; `in [...]`; `eq [...]`; `ne` is a
// word operator CREST parses and DS refuses; `true`, `false`, `3.5` parse;
// `\"` parses; an unknown field is 200 with no rows; `!(...)` parses.
// Structurally certain: input ending where a token is required, an
// unterminated string, a `)` with no `(`, a `)` or `(` where a pointer
// operator or value must be. Not measured, so `unsupported`: an empty filter,
// a non-string one, keyword letter case (`AND`, `TRUE`, `PR`), single-quoted
// strings, numbers outside JSON's grammar (`+5`, `05`, `1.2.3`), `!` not
// followed by `(`, trailing text, a quoted pointer, `null`/arrays after any
// operator but the measured ones, and pointers holding `[]{}` or a bad `~`.
//
// Also `unsupported`, from E19 fix round 3, because the on-prem CREST 27.1.1
// parser (QueryFilters.parse, run by the round-3 reviewer over 112 inputs)
// rejects them and AIC is unmeasured. The jar is a hypothesis here, not an AIC
// measurement, so these are faults, not catchable: an exponent with no `.`
// (`1e3`; CREST hands a value without `.` to Long.parseLong), an integer
// beyond 2^53 - 1 (past Java long CREST refuses it; below that and past 2^53
// a JS number cannot hold the long CREST compares), any separator but U+0020
// (CREST splits on space and parentheses only, so a tab or newline is part of
// a token there), and a `!` glued to the word or string before it (`and!(`).
var __rhinoLocalCrestFilter = (function () {
  var NUMBER = /^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$/;
  // Written out rather than `\s`, whose membership differs between Rhino and
  // V8 at the edges; both lanes must give one verdict.
  var OTHER_SPACE = /[\t\n\v\f\r\u0085\u00a0\u1680\u180e\u2000-\u200a\u2028\u2029\u202f\u205f\u3000\ufeff]/;

  function Rejection(kind, reason) {
    this.kind = kind;
    this.reason = reason;
  }

  function reject(kind, reason) {
    throw new Rejection(kind, reason);
  }

  function isDelimiter(ch) {
    return ch === "(" || ch === ")" || ch === "!" || ch === '"' || ch === " " || OTHER_SPACE.test(ch);
  }

  function numberValue(text) {
    if (/[eE]/.test(text) && text.indexOf(".") < 0) {
      reject("unsupported", "an exponent with no . (CREST 27.1.1 reads it as a long): " + text);
    }
    var number = Number(text);
    if (text.indexOf(".") < 0 && Math.abs(number) > 9007199254740991) {
      reject("unsupported", "an integer beyond 2^53 - 1: " + text);
    }
    return number;
  }

  function tokenize(src) {
    var tokens = [];
    var i = 0;
    var gap = true;
    while (i < src.length) {
      var ch = src.charAt(i);
      if (ch === " ") {
        i += 1;
        gap = true;
        continue;
      }
      if (OTHER_SPACE.test(ch)) {
        reject("unsupported", "a separator other than a space (CREST 27.1.1 splits on U+0020 only): " + JSON.stringify(ch));
      }
      if (ch === "!" && !gap && tokens.length > 0 && tokens[tokens.length - 1].type !== "punct") {
        reject("unsupported", "a ! with no space before it: " + tokens[tokens.length - 1].text + "!");
      }
      if (ch === "(" || ch === ")" || ch === "!") {
        tokens.push({ type: "punct", text: ch });
        i += 1;
        gap = false;
        continue;
      }
      if (ch === '"') {
        var start = i;
        i += 1;
        var closed = false;
        while (i < src.length) {
          var c = src.charAt(i);
          if (c === "\\") {
            i += 2;
          } else if (c === '"') {
            i += 1;
            closed = true;
            break;
          } else {
            i += 1;
          }
        }
        if (!closed) {
          reject("parse", "an unterminated string");
        }
        var text = src.substring(start, i);
        var value;
        try {
          value = JSON.parse(text);
        } catch (_error) {
          reject("unsupported", "a string escape outside JSON's: " + text);
        }
        var previous = tokens[tokens.length - 1];
        if (!gap && previous && previous.type === "word") {
          reject("unsupported", "a string with no space before it: " + text);
        }
        tokens.push({ type: "string", text: text, value: value });
        gap = false;
        if (i < src.length && !isDelimiter(src.charAt(i))) {
          reject("unsupported", "a string with no space after it: " + text);
        }
        continue;
      }
      var wordStart = i;
      while (i < src.length && !isDelimiter(src.charAt(i))) {
        i += 1;
      }
      tokens.push({ type: "word", text: src.substring(wordStart, i) });
      gap = false;
    }
    return tokens;
  }

  function isLiteralWord(text) {
    return text === "true" || text === "false";
  }

  function isCaseVariant(text, keyword) {
    return text !== keyword && text.toLowerCase() === keyword;
  }

  function checkPointer(text) {
    if (text === "/" || /[[\]{}]/.test(text) || /~(?![01])/.test(text)) {
      reject("unsupported", "a pointer this grammar does not read: " + text);
    }
    if (isCaseVariant(text, "true") || isCaseVariant(text, "false")) {
      reject("unsupported", "a literal in another letter case: " + text);
    }
  }

  function parse(input) {
    if (typeof input !== "string") {
      return { error: "unsupported", reason: "a non-string _queryFilter (" + (input === null ? "null" : typeof input) + ")" };
    }
    if (!/[^ ]/.test(input)) {
      return { error: "unsupported", reason: "an empty _queryFilter" };
    }
    var tokens;
    var pos = 0;

    function peek() {
      return tokens[pos];
    }

    function next() {
      var token = tokens[pos];
      pos += 1;
      return token;
    }

    function isPunct(token, text) {
      return token !== undefined && token.type === "punct" && token.text === text;
    }

    function isWord(token, text) {
      return token !== undefined && token.type === "word" && token.text === text;
    }

    function value(op) {
      var token = next();
      if (token === undefined) {
        reject("parse", "no value after " + op.text);
      }
      if (token.type === "string") {
        return token.value;
      }
      if (token.type === "punct") {
        if (token.text === ")") {
          reject("parse", "no value before )");
        }
        reject("unsupported", "a " + token.text + " where a value belongs");
      }
      var text = token.text;
      if (isLiteralWord(text)) {
        return text === "true";
      }
      if (text === "null" && op.text === "eq") {
        reject("parse", "eq null");
      }
      if (text.charAt(0) === "[" && (op.text === "eq" || op.text === "in")) {
        reject("parse", "an array value");
      }
      if (NUMBER.test(text)) {
        return numberValue(text);
      }
      return reject("unsupported", "a value this grammar does not read: " + text);
    }

    function primary() {
      var token = next();
      if (token === undefined) {
        reject("parse", "the filter ends where an expression belongs");
      }
      if (token.type === "punct") {
        if (token.text === "(") {
          var inner = or();
          var close = next();
          if (close === undefined) {
            reject("parse", "an unclosed (");
          }
          if (close.type !== "punct" || close.text !== ")") {
            reject("unsupported", "text this grammar does not read before ): " + close.text);
          }
          return inner;
        }
        if (token.text === ")") {
          reject("parse", "a ) where an expression belongs");
        }
        return reject("unsupported", "a ! inside an expression");
      }
      if (token.type === "string") {
        reject("unsupported", "a quoted pointer: " + token.text);
      }
      if (isLiteralWord(token.text)) {
        return { kind: token.text };
      }
      checkPointer(token.text);
      var op = next();
      if (op === undefined) {
        reject("parse", "no operator after " + token.text);
      }
      if (op.type === "punct") {
        if (op.text === "(" || op.text === ")") {
          // `not (...)` is measured: CREST reads `not` as a pointer and the
          // `(` where its operator belongs is what fails.
          reject("parse", "a " + op.text + " where an operator belongs");
        }
        reject("unsupported", "a ! where an operator belongs");
      }
      if (op.type === "string") {
        reject("unsupported", "a string where an operator belongs");
      }
      if (op.text === "pr") {
        return { kind: "pr", field: token.text };
      }
      if (isCaseVariant(op.text, "pr")) {
        reject("unsupported", "pr in another letter case: " + op.text);
      }
      var node = { kind: "cmp", field: token.text, op: op.text, value: value(op) };
      // Round-5 P2-2: the literal's spelling, since `7.0` and `7` read alike
      // as JS numbers but may not reach DS alike.
      var literal = tokens[pos - 1];
      if (typeof node.value === "number" && literal.type === "word") {
        node.spelling = literal.text;
      }
      return node;
    }

    function not() {
      if (isPunct(peek(), "!")) {
        next();
        var after = peek();
        if (after === undefined) {
          reject("parse", "the filter ends after !");
        }
        if (!isPunct(after, "(")) {
          reject("unsupported", "! not followed by (");
        }
        return { kind: "not", inner: primary() };
      }
      return primary();
    }

    function and() {
      var left = not();
      while (isWord(peek(), "and")) {
        next();
        left = { kind: "and", left: left, right: not() };
      }
      return left;
    }

    function or() {
      var left = and();
      while (isWord(peek(), "or")) {
        next();
        left = { kind: "or", left: left, right: and() };
      }
      return left;
    }

    try {
      tokens = tokenize(input);
      var ast = or();
      var rest = peek();
      if (rest !== undefined) {
        if (isPunct(rest, ")")) {
          reject("parse", "a ) with no (");
        }
        reject("unsupported", "trailing text: " + rest.text);
      }
      return { ast: ast };
    } catch (error) {
      if (error instanceof Rejection) {
        return { error: error.kind, reason: error.reason };
      }
      throw error;
    }
  }

  // RFC 6901: a leading "/" is optional here, as CREST accepts both.
  function pointerParts(path) {
    var text = String(path);
    if (text.charAt(0) === "/") {
      text = text.substring(1);
    }
    var parts = text.split("/");
    var i;
    for (i = 0; i < parts.length; i += 1) {
      parts[i] = parts[i].replace(/~1/g, "/").replace(/~0/g, "~");
    }
    return parts;
  }

  // ---------------------------------------------------------------------
  // Evaluation, shared by both lanes (E19 fix round 4). Each answer is true,
  // false, or { unknown: { field, op, dimension } }: a leaf whose answer
  // turns on something nobody measured. `and`, `or` and `!` are Kleene, so a
  // record's verdict is unknown only when the unknown leaf decides it, and
  // the lane then refuses the whole query. A lane supplies its string rule
  // (`strings`), whether the measured quoted integer holds (`quotedInteger`)
  // and a label for a decided non-string leaf (`scalar`), because only the
  // IDM emulator knows which types and fields were measured.
  var SAFE_INTEGER = 9007199254740991;
  var ORDERING = { gt: true, ge: true, lt: true, le: true };

  function unknown(field, op, dimension) {
    return { unknown: { field: field, op: op, dimension: dimension } };
  }

  function isArray(value) {
    return Object.prototype.toString.call(value) === "[object Array]";
  }

  // `throughArray` is set when a pointer step meets an array: doc 10 measured
  // only the bare positive `/<rel>/_refResourceId eq` form (no rows), and
  // whether that is the leaf being false or the query matching nothing is
  // unknown (round-5 P3-4).
  function walk(record, path) {
    var parts = pointerParts(path);
    var current = record;
    var i;
    for (i = 0; i < parts.length; i += 1) {
      if (isArray(current)) {
        // An index (or `length`) the array holds was read as that element
        // before round 5; nothing measured it either, so it is never decided.
        return { value: undefined, throughArray: true,
          own: Object.prototype.hasOwnProperty.call(current, parts[i]) };
      }
      if (current === null || current === undefined || typeof current !== "object" ||
          !Object.prototype.hasOwnProperty.call(current, parts[i])) {
        return { value: undefined, throughArray: false };
      }
      current = current[parts[i]];
    }
    return { value: current, throughArray: false };
  }

  function readField(record, path) {
    var parts = pointerParts(path);
    var current = record;
    var i;
    for (i = 0; i < parts.length; i += 1) {
      if (current === null || current === undefined || typeof current !== "object" ||
          !Object.prototype.hasOwnProperty.call(current, parts[i])) {
        return undefined;
      }
      current = current[parts[i]];
    }
    return current;
  }

  function isEmptyObject(value) {
    if (value === null || typeof value !== "object" || isArray(value)) {
      return false;
    }
    var key;
    for (key in value) {
      if (Object.prototype.hasOwnProperty.call(value, key)) {
        return false;
      }
    }
    return true;
  }

  function isSafeInteger(value) {
    return typeof value === "number" && Math.floor(value) === value && Math.abs(value) <= SAFE_INTEGER;
  }

  // The one measured type mismatch (docs/api/10, Query filter encoding):
  // `frIndexedInteger1 eq "7"` matches an integer field holding 7. Only the
  // canonical spelling of a safe integer is that case; "07", "7.0", " 7",
  // "1e1" and anything past 2^53 - 1 are not, and stay unknown.
  function canonicalInteger(text) {
    var number = Number(text);
    return isSafeInteger(number) && String(number) === text;
  }

  function compareText(left, op, right) {
    switch (op) {
      case "eq": return left === right;
      case "co": return left.indexOf(right) !== -1;
      case "sw": return left.indexOf(right) === 0;
      case "gt": return left > right;
      case "ge": return left >= right;
      case "lt": return left < right;
      case "le": return left <= right;
    }
    return false;
  }

  // One scalar against the literal. Same JSON type is decided, and the lane
  // labels it (`lane.scalar`); a different one is decided only in the
  // measured case above, and only where the lane says it holds
  // (`lane.quotedInteger`, round-5 P2-1). `co`/`sw` on a number or boolean,
  // ordering on a boolean, and a decimal literal against a number (round-5
  // P2-2: `7.0` may reach an LDAP integer attribute as text it refuses) are
  // unmeasured too.
  function compareScalar(actual, op, expected, field, lane, decimal, positive) {
    if (actual === null || typeof actual === "object") {
      return unknown(field, op, "type");
    }
    var rule = lane.rule ? lane.rule(field) : null;
    if (rule === "dsRefused") {
      return unknown(field, op, "rule");
    }
    if (rule !== null) {
      var decided = compareByRule(rule, actual, op, expected, field, lane, positive);
      if (decided !== undefined) {
        return decided;
      }
    }
    var actualType = typeof actual;
    var expectedType = typeof expected;
    if (actualType === "string" && expectedType === "string") {
      return lane.strings(field, op, actual, expected);
    }
    if (actualType === expectedType) {
      if (actualType === "number" && decimal) {
        return unknown(field, op, "type");
      }
      if (op === "eq" || (actualType === "number" && ORDERING[op])) {
        lane.scalar(field, op, actualType);
        return op === "eq" ? actual === expected : compareText(actual, op, expected);
      }
      return unknown(field, op, "type");
    }
    if (op === "eq" && actualType === "number" && expectedType === "string" &&
        isSafeInteger(actual) && canonicalInteger(expected) && lane.quotedInteger(field)) {
      return actual === Number(expected);
    }
    return unknown(field, op, "type");
  }

  function compareField(record, node, lane) {
    var field = node.field;
    var op = node.kind === "pr" ? "pr" : node.op;
    var expected = node.kind === "pr" ? null : node.value;
    var decimal = node.spelling !== undefined && /[.eE]/.test(node.spelling);
    var read = walk(record, field);
    if (read.throughArray) {
      return op === "eq" && node.positive === true && !read.own ? false : unknown(field, op, "path");
    }
    var value = read.value;
    if (op === "pr") {
      if (value === undefined || value === null) {
        return false;
      }
      // Whether DS stores an empty string, array or object as present is
      // unmeasured (round-5 P3-3 added `{}`).
      if (isArray(value) && value.length === 0 && lane.rule && lane.rule(field) === "emptyAbsent") {
        lane.label("measured", "pr reads an explicitly written [] on alpha_user " + fieldName(field) + " as absent", PROBE);
        return false;
      }
      if (value === "" || (isArray(value) && value.length === 0) || isEmptyObject(value)) {
        return unknown(field, op, "presence");
      }
      return true;
    }
    if (value === undefined || value === null) {
      return false;
    }
    // AIC parses an escaped quote but never matches it: `sn eq "Twelve\" B"`
    // is 200 with no rows against a record that holds it (docs/api/10). Only
    // this bare positive `eq` is decided; `prepare` refuses every other place
    // a `"` can sit, where "the leaf is false" and "the query matches
    // nothing" give different answers.
    if (op === "eq" && typeof expected === "string" && expected.indexOf('"') !== -1) {
      return false;
    }
    if (!isArray(value)) {
      return compareScalar(value, op, expected, field, lane, decimal, node.positive === true);
    }
    // Any-match: a definite element match wins; otherwise any undecided
    // element leaves the field undecided.
    var undecided = null;
    var i;
    for (i = 0; i < value.length; i += 1) {
      var one = compareScalar(value[i], op, expected, field, lane, decimal, node.positive === true);
      if (one === true) {
        return true;
      }
      if (one !== false && undecided === null) {
        undecided = one;
      }
    }
    return undecided === null ? false : undecided;
  }

  // `lane` is { strings, quotedInteger, scalar, rule, admits, label }: see
  // `scriptTesterLane` for the script tester's and src/idm/filter.ts for the IDM emulator's.
  function evaluate(ast, record, lane) {
    switch (ast.kind) {
      case "true": return true;
      case "false": return false;
      case "not": {
        var inner = evaluate(ast.inner, record, lane);
        return inner === true ? false : inner === false ? true : inner;
      }
      case "and": {
        var andLeft = evaluate(ast.left, record, lane);
        if (andLeft === false) {
          return false;
        }
        var andRight = evaluate(ast.right, record, lane);
        return andRight === false ? false : andLeft === true ? andRight : andLeft;
      }
      case "or": {
        var orLeft = evaluate(ast.left, record, lane);
        if (orLeft === true) {
          return true;
        }
        var orRight = evaluate(ast.right, record, lane);
        return orRight === true ? true : orLeft === false ? orRight : orLeft;
      }
      case "pr":
      case "cmp": return compareField(record, ast, lane);
    }
    return false;
  }

  // parse, then what a lane can evaluate. `evaluated` lists the operators the
  // lane evaluates; `ne` is reported apart because its REST refusal is
  // measured. Also refuses an escaped quote anywhere but a bare positive `eq`
  // reached through `and` alone (round-4 P3-2): whether AIC's measured "no
  // rows" is that leaf being false or the whole query matching nothing is
  // unknown, and only on that path do the two agree.
  // `evaluated` is the operators the lane evaluates, or a predicate on the
  // leaf (the script tester's admits ordering on some fields only).
  function prepare(input, evaluated) {
    var parsed = parse(input);
    if (parsed.error) {
      return parsed;
    }
    var admits = typeof evaluated === "function" ? evaluated : function (node) {
      return evaluated.indexOf(node.op) !== -1;
    };
    var reason = null;
    var ne = false;
    (function check(node, positive) {
      if (node.kind === "and") {
        check(node.left, positive);
        check(node.right, positive);
      } else if (node.kind === "or") {
        check(node.left, false);
        check(node.right, false);
      } else if (node.kind === "not") {
        check(node.inner, false);
      } else if (node.kind === "pr") {
        node.positive = positive;
      } else if (node.kind === "cmp") {
        node.positive = positive;
        if (node.op === "ne") {
          ne = true;
        } else if (!admits(node) && reason === null) {
          reason = "operator " + node.op + " is not evaluated by the harness";
        }
        if (typeof node.value === "string" && node.value.indexOf('"') !== -1 && reason === null &&
            !(node.op === "eq" && positive)) {
          reason = "an escaped quote outside a bare eq reached through and alone; AIC's answer there is unmeasured";
        }
      }
    })(parsed.ast, true);
    if (reason !== null) {
      return { error: "unsupported", reason: reason };
    }
    return ne ? { error: "ne", reason: "AIC refuses ne" } : parsed;
  }

  // Readings of a string comparison AIC might apply. AIC folds letter case
  // and edge spaces for some fields (docs/api/10, alpha_user) and nothing
  // measured the rest. Index 0 of each list is the identity.
  function asciiLower(text) {
    return text.replace(/[A-Z]/g, function (c) { return c.toLowerCase(); });
  }
  function asciiUpper(text) {
    return text.replace(/[a-z]/g, function (c) { return c.toUpperCase(); });
  }
  function trim(text) {
    return text.replace(/^ +| +$/g, "");
  }
  function collapse(text) {
    return trim(text).replace(/ {2,}/g, " ");
  }
  // RFC 4518 2.2 maps these to SPACE before insignificant-space handling.
  function toSpace(text) {
    return text.replace(/[\t-\r\u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]/g, " ");
  }
  // RFC 4518 2.2 maps these to nothing: soft hyphens, joiners, variation
  // selectors, zero-width space, other controls and format characters (BMP
  // only; the astral ones are not read).
  var MAPPED_TO_NOTHING = /[\u0000-\u0008\u000e-\u001f\u007f-\u0084\u0086-\u009f\u00ad\u034f\u06dd\u070f\u1806\u180b-\u180e\u200b-\u200f\u202a-\u202e\u2060-\u2063\u206a-\u206f\ufe00-\ufe0f\ufeff\ufff9-\ufffc]/g;
  function mapToNothing(text) {
    return text.replace(MAPPED_TO_NOTHING, "");
  }
  // RFC 4518 2.3: NFKC. Rhino 1.7.14 and V8 both have String.prototype.normalize.
  function nfkc(text) {
    return text.normalize("NFKC");
  }
  function identity(text) {
    return text;
  }

  var transforms = {
    identity: identity,
    asciiLower: asciiLower,
    asciiUpper: asciiUpper,
    fullLower: function (t) { return t.toLowerCase(); },
    foldLower: function (t) { return t.toUpperCase().toLowerCase(); },
    // Round-5 P3-1: Java's per-char Character.toLowerCase (U+0130 to `i`,
    // not `i` + U+0307), and a full fold (U+1E9E to `ss`, RFC 3454 B.2).
    charLower: function (t) { return t.replace(/[\s\S]/g, function (c) { return c.toLowerCase().charAt(0); }); },
    fullFold: function (t) { return t.toLowerCase().toUpperCase().toLowerCase(); },
    trim: trim,
    collapse: collapse,
    wideTrim: function (t) { return trim(toSpace(t)); },
    wideCollapse: function (t) { return collapse(toSpace(t)); },
    mapToNothing: mapToNothing,
    nfkc: nfkc,
    nfkcMapped: function (t) { return nfkc(mapToNothing(t)); },
  };
  var unicodeReadings = [identity, mapToNothing, nfkc, transforms.nfkcMapped];

  // The answer under the first case and space reading (the lane's applied
  // rule), and the dimension of the first reading that disagrees, if any.
  function readStrings(candidate, op, expected, cases, spaces) {
    function run(c, s, u) {
      return compareText(c(s(u(candidate))), op, c(s(u(expected))));
    }
    var answer = run(cases[0], spaces[0], identity);
    var c, s, u;
    for (u = 0; u < unicodeReadings.length; u += 1) {
      for (c = 0; c < cases.length; c += 1) {
        for (s = 0; s < spaces.length; s += 1) {
          if (run(cases[c], spaces[s], unicodeReadings[u]) === answer) {
            continue;
          }
          var dimension = run(cases[c], spaces[0], identity) !== answer ? "case"
            : run(cases[c], spaces[s], identity) !== answer ? "space" : "unicode";
          return { answer: answer, dimension: dimension };
        }
      }
    }
    return { answer: answer, dimension: null };
  }

  var everyCase = [identity, asciiLower, transforms.fullLower, asciiUpper, transforms.foldLower, transforms.charLower,
    transforms.fullFold];
  var everySpace = [identity, trim, collapse, transforms.wideTrim, transforms.wideCollapse];

  // The rule for a lane with no per-field policy: a leaf whose answer moves
  // under any reading is unknown.
  function unmeasuredStrings(field, op, candidate, expected) {
    var read = readStrings(candidate, op, expected, everyCase, everySpace);
    return read.dimension === null ? read.answer : unknown(field, op, read.dimension);
  }

  // ---------------------------------------------------------------------
  // Field rules (E19 round 5 named them; the live probe of 2026-11-06,
  // docs/api/10, measured the alpha_user ones). DS's core schema (opendj-core
  // 8.1.1, RFC 4519) gives telephoneNumber EQUALITY telephoneNumberMatch, and
  // frIndexedDate1-5 / frUnindexedDate1-5 are fr-attr-idateN / fr-attr-dateN,
  // GeneralizedTime (docs/api/14); frIndexedInteger1-5 / frUnindexedInteger1-5
  // are fr-attr-iintN / fr-attr-intN, integers. No string reading describes
  // the first two. Measured, on alpha_user, over REST and IDM's openidm.query
  // (and, for the rows the second probe ran, AM's):
  //   phone        telephoneNumber: spaces, hyphens and `+` ignored anywhere,
  //                in the stored value and the literal, for eq, sw and co
  //                (`+` settled by the second probe, 2026-11-06: `eq
  //                "61499999999"`, `eq "6+1499999999"` and `co "+9999"` all
  //                match `+614-9999-9999`; `eq "61499999998"` does not).
  //   instant      frIndexedDate1, frUnindexedDate1: eq, gt and lt compare
  //                instants; `eq "notadate"` is a 400.
  //   integer      frIndexedInteger1: an integral literal of either JSON type
  //                equals the integer; 7.5, "7.5" and "abc" are a 400.
  //   emptyAbsent  frIndexedMultivalued1, frUnindexedMultivalued1, roles: an
  //                explicitly written [] is not present.
  // The other members of each family, and every one on bravo_user, were not
  // probed: dsInteger (an integer attribute whose 400s are unmeasured) and
  // dsRefused (no comparison is decided). Every lane applies the measured
  // rules: the script tester's openidm.query was measured to return the same
  // rows (second probe, 2026-11-06), so its lane decides them too. It admits
  // ordering only on the instant fields, the one ordering measured through
  // that binding.
  var PROBE = "docs/api/10-managed-objects.md (E19 live probe, 2026-11-06)";
  var PHONE_OPS = { eq: true, sw: true, co: true };
  var INSTANT_OPS = { eq: true, gt: true, lt: true };
  var DECIDED_INTEGER_OPS = { eq: true, gt: true, ge: true, lt: true, le: true };
  var ILLEGAL_NUMBER_OPS = { eq: true, gt: true, lt: true };

  function fieldName(field) {
    return String(field).replace(/^\//, "");
  }

  function fieldRule(type, field) {
    if (type !== "alpha_user" && type !== "bravo_user") {
      return null;
    }
    var f = fieldName(field);
    var alpha = type === "alpha_user";
    if (f === "telephoneNumber") {
      return alpha ? "phone" : "dsRefused";
    }
    if (/^fr(?:Indexed|Unindexed)Date[1-5]$/.test(f)) {
      return alpha && /Date1$/.test(f) ? "instant" : "dsRefused";
    }
    if (/^fr(?:Indexed|Unindexed)Integer[1-5]$/.test(f)) {
      return alpha && f === "frIndexedInteger1" ? "integer" : "dsInteger";
    }
    if (alpha && (f === "frIndexedMultivalued1" || f === "frUnindexedMultivalued1" || f === "roles")) {
      return "emptyAbsent";
    }
    return null;
  }

  function phoneDigits(text) {
    return text.replace(/[ +-]/g, "");
  }
  // Only the characters the probe stored and asserted: other separators
  // (`(`, `)`, `.`), other dashes and spaces, and letters were not probed.
  function phoneText(text) {
    return /^[0-9+ -]*$/.test(text);
  }

  // The measured literal forms: seconds precision, `Z` or a `+hh:mm` offset.
  var INSTANT = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:Z|([+-])(\d{2}):(\d{2}))$/;
  function instant(text) {
    var m = INSTANT.exec(text);
    if (m === null) {
      return null;
    }
    var n = [];
    var i;
    for (i = 1; i <= 9; i += 1) {
      n.push(m[i] === undefined || m[i] === "+" || m[i] === "-" ? m[i] : Number(m[i]));
    }
    var ms = Date.UTC(n[0], n[1] - 1, n[2], n[3], n[4], n[5]);
    var check = new Date(ms);
    if (n[1] < 1 || n[1] > 12 || check.getUTCDate() !== n[2] || n[3] > 23 || n[4] > 59 || n[5] > 59 ||
        (n[6] !== undefined && (n[7] > 23 || n[8] > 59))) {
      return null;
    }
    if (n[6] !== undefined) {
      ms -= (n[6] === "-" ? -1 : 1) * (n[7] * 60 + n[8]) * 60000;
    }
    return ms;
  }

  // A quoted literal: how IDM converts one to the integer syntax is
  // unmeasured, so only the probed spellings are decided. `"7.0"` is
  // integral; `"7.5"` is fractional, and decided only when it is the shortest
  // spelling of its Double (trailing zeros aside), so `"7.0000000000000001"`,
  // which a Double parse would make 7, is neither (review pf-02 P3-1).
  var DECIMAL = /^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?$/;
  function integralDecimal(text) {
    return /^-?(?:0|[1-9][0-9]*)(?:\.0+)?$/.test(text) && Math.abs(Number(text)) <= SAFE_INTEGER;
  }
  function fractionalDecimal(text) {
    if (!/^-?(?:0|[1-9][0-9]*)\.[0-9]*[1-9]0*$/.test(text)) {
      return false;
    }
    var number = Number(text);
    return Math.floor(number) !== number && String(number) === text.replace(/0+$/, "");
  }

  function compareNumbers(actual, op, expected) {
    switch (op) {
      case "eq": return actual === expected;
      case "gt": return actual > expected;
      case "ge": return actual >= expected;
      case "lt": return actual < expected;
      case "le": return actual <= expected;
    }
    return undefined;
  }

  // A deciding lane's answer for a field with a measured rule, or undefined
  // to fall through to the ordinary comparison. `assertions` has already
  // refused or 400'd every literal these do not decide.
  function compareByRule(rule, actual, op, expected, field, lane, positive) {
    var f = fieldName(field);
    if (rule === "phone") {
      if (typeof actual !== "string" || typeof expected !== "string") {
        return unknown(field, op, "type");
      }
      if (!phoneText(actual)) {
        return unknown(field, op, "rule");
      }
      lane.label("measured", op + " on alpha_user telephoneNumber ignores spaces, hyphens and + (telephoneNumberMatch)", PROBE);
      return compareText(phoneDigits(actual), op, phoneDigits(expected));
    }
    if (rule === "instant") {
      if (typeof actual !== "string" || typeof expected !== "string") {
        return unknown(field, op, "type");
      }
      var stored = instant(actual);
      if (stored === null) {
        return unknown(field, op, "rule");
      }
      lane.label("measured", op + " on alpha_user " + f + " compares instants across offsets", PROBE);
      return compareNumbers(stored, op, instant(expected));
    }
    if (rule === "integer") {
      if (!isSafeInteger(actual)) {
        return unknown(field, op, "type");
      }
      var literal = typeof expected === "number" ? expected : Number(expected);
      var measured = op === "eq" || (op === "ge" && typeof expected === "number");
      lane.label(measured ? "measured" : "assumed", measured
        ? op + " on alpha_user frIndexedInteger1 converts an integral literal of either JSON type to the integer"
        : op + " on alpha_user frIndexedInteger1 compares an integral literal numerically, like eq and ge", PROBE);
      return compareNumbers(actual, op, literal);
    }
    // `jsonOwn`: a type with its own genericMapping entry. The probe's type had
    // none (`managed/*` alone); an own entry's JSON keys and matching rule
    // were not measured, so the same answers are labelled assumed.
    if ((rule === "json" || rule === "jsonOwn") && typeof actual === "number") {
      var own = rule === "jsonOwn";
      var ownNote = own ? ", assumed for a type with its own genericMapping entry like managed/*" : "";
      if (typeof expected === "string") {
        if (positive && INSTANT_OPS[op]) {
          lane.label(own ? "assumed" : "measured", op + " on a generic (JSON-stored) type: a quoted literal never matches a number" + ownNote, PROBE);
          return false;
        }
        return unknown(field, op, "type");
      }
      if (typeof expected === "number" && DECIDED_INTEGER_OPS[op]) {
        var probed = op === "eq" || op === "gt";
        lane.label(probed && !own ? "measured" : "assumed", probed
          ? op + " on a generic (JSON-stored) type compares numbers by value, decimals included" + ownNote
          : op + " on a generic (JSON-stored) type compares numbers by value, like eq and gt" + ownNote, PROBE);
        return compareNumbers(actual, op, expected);
      }
    }
    return undefined;
  }

  // Filter-level: the leaves AIC answers before reading a record. A literal a
  // measured rule refuses is AIC's 400 (`illegal`); any other literal on a
  // ruled field that the lane cannot decide is `unknown`, so an empty
  // collection cannot pass where AIC would throw. Unknown wins over illegal,
  // because which leaf AIC names first is unmeasured.
  function assertions(ast, lane) {
    var found = null;
    var illegal = null;
    (function visit(node) {
      if (found !== null) {
        return;
      }
      if (node.kind === "and" || node.kind === "or") {
        visit(node.left);
        visit(node.right);
        return;
      }
      if (node.kind === "not") {
        visit(node.inner);
        return;
      }
      if (node.kind !== "cmp" || !lane.rule) {
        return;
      }
      var rule = lane.rule(node.field);
      var verdict = rule === null ? null : leafAssertion(rule, node);
      if (verdict === null) {
        return;
      }
      if (verdict.unknown) {
        found = verdict;
      } else if (illegal === null) {
        illegal = verdict;
      }
    })(ast);
    return found !== null ? found : illegal;
  }

  function leafAssertion(rule, node) {
    var op = node.op;
    var value = node.value;
    var field = node.field;
    var plainInteger = typeof value === "number" && /^-?(?:0|[1-9][0-9]*)$/.test(node.spelling);
    var illegal = function (text) {
      return { illegal: { field: "/" + fieldName(field), value: text, op: op } };
    };
    switch (rule) {
      case "dsRefused":
        return unknown(field, op, "rule");
      case "phone":
        // A literal with no digit strips to "" (`co "+"` would match every
        // number): unmeasured.
        return PHONE_OPS[op] && typeof value === "string" && phoneText(value) && /[0-9]/.test(value)
          ? null : unknown(field, op, "rule");
      case "instant":
        if (typeof value !== "string") {
          return unknown(field, op, "type");
        }
        if (instant(value) !== null && INSTANT_OPS[op]) {
          return null;
        }
        return op === "eq" && /^[A-Za-z]+$/.test(value) ? illegal(value) : unknown(field, op, "rule");
      case "integer":
        if (!DECIDED_INTEGER_OPS[op]) {
          return unknown(field, op, "type");
        }
        if (typeof value === "number") {
          // CREST has already parsed the literal to a Long or a Double, so DS
          // sees the value, not the spelling: `2.00000000000000004` is the
          // Double 2.0, the same filter as `2.0` (review pf-02 P3-1). The
          // spelling only has to be one of the probed forms (no exponent).
          if (!DECIMAL.test(node.spelling)) {
            return unknown(field, op, "type");
          }
          if (isSafeInteger(value)) {
            return null;
          }
          var abs = Math.abs(value);
          return Math.floor(value) !== value && ILLEGAL_NUMBER_OPS[op] && abs >= 1e-3 && abs < 1e7
            ? illegal(String(value)) : unknown(field, op, "type");
        }
        if (typeof value === "string" && op === "eq") {
          if (integralDecimal(value)) {
            return null;
          }
          return fractionalDecimal(value) || /^[A-Za-z]+$/.test(value) ? illegal(value) : unknown(field, op, "type");
        }
        return unknown(field, op, "type");
      case "dsInteger":
        if (plainInteger && DECIDED_INTEGER_OPS[op]) {
          return null;
        }
        return op === "eq" && typeof value === "string" && canonicalInteger(value) ? null : unknown(field, op, "type");
    }
    return null;
  }

  // The script tester's lane: every reading and no measured type coercion,
  // for a collection it knows by name only. The measured alpha_user field
  // rules do apply: AM's openidm.query returned exactly the REST and IDM
  // rows on every probed rule (second probe, 2026-11-06, docs/api/12). Its
  // grammar evaluates eq, co and sw, plus ordering on an instant field (gt
  // and lt were measured through this binding; `assertions` refuses ge/le).
  function scriptTesterLane(collection) {
    var type = String(collection).replace(/^managed\//, "");
    return {
      strings: unmeasuredStrings,
      quotedInteger: function () { return false; },
      scalar: function () {},
      rule: function (field) { return fieldRule(type, field); },
      admits: function (node) {
        return node.op === "eq" || node.op === "co" || node.op === "sw" ||
          (ORDERING[node.op] === true && fieldRule(type, node.field) === "instant");
      },
      label: function () {},
    };
  }

  return {
    parse: parse,
    pointerParts: pointerParts,
    prepare: prepare,
    evaluate: evaluate,
    readField: readField,
    readStrings: readStrings,
    unmeasuredStrings: unmeasuredStrings,
    scriptTesterLane: scriptTesterLane,
    fieldRule: fieldRule,
    assertions: assertions,
    compareText: compareText,
    transforms: transforms,
  };
})();

// CommonJS in Node; the same text runs as a global-scope script in Rhino.
if (typeof module !== "undefined" && module.exports) {
  module.exports = __rhinoLocalCrestFilter;
}
