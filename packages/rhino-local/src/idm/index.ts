/** Experimental IDM core. Import from source until its REST/provenance contract settles. */
export { IdmConfig } from "./config.ts";
export { IdmEmulator } from "./emulator.ts";
export { RUNTIME_SCRIPT_FAULT, RUNTIME_SCRIPT_PATH_FAULT, RUNTIME_SCRIPT_PROVIDER_FAULT, RuntimeScripts, type RuntimeScriptProvider } from "./runtime-scripts.ts";
export type { ConfigInput, Document, EndpointConfig, ManagedConfig, ManagedType,
  PolicyEvaluator, Provenance, RestRequest, RestResult, Script } from "./types.ts";
