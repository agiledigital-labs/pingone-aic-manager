/**
 * AIC wrapper-journey lane. One case definition (`src/case`) drives this
 * lane and the local Rhino lane; both are judged by `judge()` in verdict.ts.
 */
export { HARNESS_CALLBACK_ID, FAILURE_NODE_ID, SUCCESS_NODE_ID } from "./constants.ts";
export { parseAuthenticateCallbacks } from "./callbacks.ts";
export type { AuthenticateCallbacks } from "./callbacks.ts";
export { conform } from "./conform.ts";
export type {
  ConformanceInput,
  ConformanceReport,
  LaneResult,
  LaneRunner,
} from "./conform.ts";
export { diffRecordedEffects, judgeBoth } from "./diff.ts";
export type {
  EffectsComparison,
  EffectsDisagreement,
  ObservationGap,
} from "./diff.ts";
export { emitWrapperJourney, subjectOutcomes } from "./emit-journey.ts";
export type {
  EmitJourneyOptions,
  NodeResource,
  ScriptResource,
  WrapperInvoke,
  WrapperJourney,
} from "./emit-journey.ts";
export { emitResultScript } from "./emit-result.ts";
export { instrumentSubject } from "./emit-subject.ts";
export type { InstrumentedSubject } from "./emit-subject.ts";
export { emitSetupScript } from "./emit-setup.ts";
export { assembleEffects, classifyFinal, parseSubjectDump } from "./record.ts";
export type { SubjectDump } from "./record.ts";
export { runAicLane } from "./run.ts";
export type { RunAicOptions } from "./run.ts";
export { AicLaneError, connectTenant, defaultAicIo } from "./tenant.ts";
export type { AicIo, TenantSession } from "./tenant.ts";
export { aicUnsupportedReason } from "./unsupported.ts";
