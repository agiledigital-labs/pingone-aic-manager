// Shared widget vocabulary: the id format, the status set, and the storage key.
//
// This module is imported by BOTH demo endpoints and is bundled into each of
// them. That is the whole point of the project — before it, sharing this much
// between two IDM endpoints meant either copy-pasting it into both `source`
// strings or paying an `openidm.action` hop to a third endpoint
// (docs/sharing-code-between-am-and-idm.md).
//
// User file — yours to change. `workspace update` refreshes this only if you have not edited it.

import { v, type Validator } from "../../framework/index.ts";

/** Widget ids are `w-` plus 4..12 lowercase alphanumerics. */
export const WIDGET_ID_PATTERN = /^w-[a-z0-9]{4,12}$/;

export const WIDGET_STATUSES = ["active", "retired", "draft"] as const;

export type WidgetStatus = (typeof WIDGET_STATUSES)[number];

/** The path/query validator for a widget id. Reused by both endpoints. */
export const widgetId = (): Validator<string> =>
  v.string({
    pattern: WIDGET_ID_PATTERN,
    patternDescription: "be a widget id of the form w-xxxx",
    description: "Widget identifier.",
  });

export const widgetStatus = (): Validator<WidgetStatus> =>
  v.enumOf(WIDGET_STATUSES, "Lifecycle state of the widget.");

/**
 * The canonical storage key for a widget. Both endpoints derive it the same
 * way, so a report and a read can never disagree about which record they mean.
 */
export function widgetKey(id: string): string {
  return "widget/" + id;
}

/** `true` when `value` is a well-formed widget id. */
export function isWidgetId(value: string): boolean {
  return WIDGET_ID_PATTERN.test(value);
}

// Response declarations are VALIDATORS, not `.schema`: passing the validator to
// a route is what type-checks the handler's return value against it. A query
// route declares one ROW and the framework wraps the envelope, so there is no
// separate `*_QUERY_RESPONSE` to keep in step.
export const WIDGET_RESPONSE = v.object(
  {
    _id: widgetId(),
    name: v.string(),
    status: widgetStatus(),
    tags: v.list(v.string()),
    metadata: v.record(v.string()),
    owner: v.optional(v.string()),
    ownerDetail: v.optional(v.unknownValue()),
    history: v.optional(v.list(v.unknownValue())),
    retiredReason: v.optional(v.string()),
    _patchOperations: v.optional(v.integer()),
  },
  { description: "A widget resource." }
);
export const IMPORT_RESPONSE = v.object({
  imported: v.integer(),
  names: v.list(v.string()),
});
export const DAILY_RESPONSE = v.object({
  _id: v.string(), date: v.isoDate(), created: v.integer(),
  retired: v.integer(), active: v.integer(),
});
export const SUMMARY_RESPONSE = v.object({
  _id: v.string(), widgetId: widgetId(), status: widgetStatus(),
  tagCount: v.integer(), events: v.integer(),
});
export const REPORT_ROW_RESPONSE = v.object({
  _id: v.string(),
  bucket: v.string(),
  count: v.integer(),
});
