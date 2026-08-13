// Shared widget vocabulary: the id format, the status set, and the storage key.
//
// This module is imported by BOTH demo endpoints and is bundled into each of
// them. That is the whole point of the project — before it, sharing this much
// between two IDM endpoints meant either copy-pasting it into both `source`
// strings or paying an `openidm.action` hop to a third endpoint
// (docs/sharing-code-between-am-and-idm.md).
//
// User file — seeded once, yours to change.

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
