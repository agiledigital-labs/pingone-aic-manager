import { readFileSync } from "node:fs";
import { join } from "node:path";
import { defineCase } from "../src/case/index.ts";
import { casesDir } from "../src/paths.ts";

function script(name: string): string {
  return readFileSync(join(casesDir, name), "utf8");
}

/** Read shared state, branch, set outcome via `action.goTo`. */
export const decideFromState = defineCase({
  name: "decide-from-state",
  script: script("decide-from-state.cjs"),
  given: { sharedState: { username: "alice" } },
  expect: { outcome: "true" },
});

/** Write shared + transient state, then exit. */
export const writeState = defineCase({
  name: "write-state",
  script: script("write-state.cjs"),
  expect: {
    outcome: "true",
    sharedState: { added: { checked: true } },
    transientState: { added: { scratch: "n/a" } },
  },
});

/** `openidm.read` of a seeded managed record, then copy a field into state. */
export const openidmRead = defineCase({
  name: "openidm-read",
  script: script("openidm-read.cjs"),
  given: {
    managed: {
      "managed/alpha_user": [
        { _id: "alice", userName: "alice", mail: "alice@example.com" },
      ],
    },
  },
  expect: {
    outcome: "true",
    sharedState: { added: { mail: "alice@example.com" } },
    openidm: [{ method: "read", resource: "managed/alpha_user/alice" }],
    logs: [{ level: "info", message: "loaded alice from openidm" }],
  },
});

export const cases = [decideFromState, writeState, openidmRead];
