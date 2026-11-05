// The one CREST _queryFilter grammar for both lanes: the script tester's
// openidm.query (runtime.cjs, as text in Rhino and node:vm) and the IDM
// emulator (src/idm/filter.ts, as CommonJS). Neither lane may grow its own.
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
      return { kind: "cmp", field: token.text, op: op.text, value: value(op) };
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

  return { parse: parse, pointerParts: pointerParts };
})();

// CommonJS in Node; the same text runs as a global-scope script in Rhino.
if (typeof module !== "undefined" && module.exports) {
  module.exports = __rhinoLocalCrestFilter;
}
