/**
 * AIC wrapper-journey lane. The same case definitions (`src/case`) drive this
 * lane and the local Rhino lane; both are judged by `judge()` in verdict.ts.
 */
export { HARNESS_CALLBACK_ID, FAILURE_NODE_ID, SUCCESS_NODE_ID } from "./constants.ts";
export { parseAuthenticateCallbacks } from "./callbacks.ts";
export type { AuthenticateCallbacks } from "./callbacks.ts";
export { chainFromRunResult, conform, conformChain } from "./conform.ts";
export type {
  AicChainRunner,
  ChainConformanceInput,
  ChainConformanceReport,
  ChainPassReport,
  ConformanceInput,
  ConformanceReport,
  LaneResult,
  LaneRunner,
  LocalChainResult,
} from "./conform.ts";
export { diffRecordedEffects, judgeBoth } from "./diff.ts";
export type {
  EffectsComparison,
  EffectsDisagreement,
  ObservationGap,
} from "./diff.ts";
export { emitLeasedJourney, emitWrapperJourney, subjectOutcomes } from "./emit-journey.ts";
export type {
  EmitJourneyOptions,
  LeasedJourneyOptions,
  NodeResource,
  ScriptResource,
  WrapperInvoke,
  WrapperJourney,
} from "./emit-journey.ts";
export {
  AIC_LEASE_NAMESPACE,
  createLeaseIdentity,
  normalizedLeaseOutcomes,
  sha256,
  uuidV5,
} from "./lease-identity.ts";
export type {
  LeaseIdentity,
  LeaseIdentityOptions,
  LeaseResourceIds,
} from "./lease-identity.ts";
export { emitResultScript } from "./emit-result.ts";
export { emitLeasedSessionJourney } from "./emit-session.ts";
export { instrumentSubject } from "./emit-subject.ts";
export type { InstrumentedSubject } from "./emit-subject.ts";
export type { ManagedFixture } from "./managed.ts";
export { AicFileLease } from "./file-lease.ts";
export type { AicFileLeaseOptions, AicLeaseRunRequest } from "./file-lease.ts";
export {
  acquireLeaseLock,
  addJournalFixture,
  addJournalResources,
  leaseStatePaths,
  newLeaseJournal,
  readLeaseJournal,
  removeJournalFixture,
  removeLeaseJournal,
  writeLeaseJournal,
} from "./lease-lock.ts";
export type {
  LeaseJournal,
  LeaseLock,
  LeaseStatePaths,
} from "./lease-lock.ts";
export {
  confirmResourceSnapshot,
  nodeRequestProjection,
  resourceRequestProjection,
} from "./resource-snapshot.ts";
export type { AicResourceKind } from "./resource-snapshot.ts";
export { assembleEffects, classifyFinal, parseSubjectDump } from "./record.ts";
export type { SubjectDump } from "./record.ts";
export { runAicChain, runAicLane, validateAicRun } from "./run.ts";
export type { AicReply, AicRunValidation, RunAicOptions } from "./run.ts";
export {
  AicLaneError,
  AM_CONFIG_API_VERSION,
  amConfigHeaders,
  connectTenant,
  defaultAicIo,
} from "./tenant.ts";
export type { AicIo, TenantSession } from "./tenant.ts";
export { aicUnsupportedReason } from "./unsupported.ts";
