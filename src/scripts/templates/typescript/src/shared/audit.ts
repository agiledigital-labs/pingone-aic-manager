// Shared audit logging, imported by both demo endpoints.
//
// Everything here goes through `describeCaller`, which is an explicit
// allowlist. Do not "improve" it by logging `context` — it carries the bearer
// token and the session token (docs/api/11-idm-endpoints.md).
//
// User file — seeded once, yours to change.

import {
  describeCaller,
  type IdmCallContext,
  type RequestLogger,
} from "../../framework/index.ts";

export interface AuditEvent {
  /** What happened, in past tense: `widget.retired`. */
  action: string;
  /** The resource it happened to, e.g. `widget/w-abc1`. */
  subject?: string;
  /** Extra non-sensitive fields. */
  fields?: Record<string, unknown>;
}

/** Emit one audit line, tagged so `aic logs search audit=1` finds them all. */
export function audit(
  log: RequestLogger,
  context: IdmCallContext,
  event: AuditEvent
): void {
  log.info(event.action, {
    audit: 1,
    ...(event.subject === undefined ? {} : { subject: event.subject }),
    ...describeCaller(context),
    ...(event.fields ?? {}),
  });
}
