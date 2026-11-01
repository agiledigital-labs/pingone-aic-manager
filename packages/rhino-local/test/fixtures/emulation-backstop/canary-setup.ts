import { recordFrameworkImport } from "./canary-state.ts";
// First setup entry: check immediately, before any test-module work can hide a race.
recordFrameworkImport("setup");
