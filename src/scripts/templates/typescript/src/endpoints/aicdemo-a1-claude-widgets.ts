// Demo endpoint: the full CREST surface over an in-memory widget collection.
//
// Deployed as `endpoint/aicdemo-a1-claude-widgets` (the file name IS the
// endpoint name). Delete this file and its `reports` sibling once you have
// your own endpoints — they exist to show the authoring API.
//
// Every handler parameter below is inferred, not annotated: `query.limit` is
// `number` because it was declared `v.integer`, `query.status` is
// `"active" | "retired" | "draft" | undefined` because it was declared an
// optional `v.enumOf`. Change a validator and the handler stops compiling.
//
// User file — seeded once, yours to change.

import {
  badRequest,
  conflict,
  defineEndpoint,
  notFound,
  queryResult,
  route,
  v,
} from "../../framework/index.ts";
import { audit } from "../shared/audit.ts";
import { findWidget, WIDGETS, type Widget } from "../shared/fixtures.ts";
import {
  IMPORT_RESPONSE,
  WIDGET_QUERY_RESPONSE,
  WIDGET_RESPONSE,
  widgetId,
  widgetKey,
  widgetStatus,
} from "../shared/widget-key.ts";

/** Scope required for anything that mutates a widget's lifecycle. */
const WRITE_SCOPE = "aicdemo:widgets:write";

const widgetBody = v.object({
  name: v.string({ minLength: 1, maxLength: 64 }),
  status: widgetStatus(),
  tags: v.list(v.string({ minLength: 1, maxLength: 32 }), { maxItems: 20 }),
  metadata: v.optional(v.record(v.string({ maxLength: 256 }))),
});

const widgetIdParams = { widgetId: widgetId() };

function requireWidget(id: string): Widget {
  const found = findWidget(id);
  if (found === undefined) {
    throw notFound("No widget " + id, { key: widgetKey(id) });
  }
  return found;
}

export default defineEndpoint({
  name: "aicdemo-a1-claude-widgets",
  summary: "Widgets (demo)",
  // Validated on every route; also the log correlation id when present.
  headers: { "x-request-id": v.optional(v.uuid()) },
  routes: [
    route({
      method: "query",
      path: "/",
      query: {
        status: v.optional(widgetStatus()),
        limit: v.withDefault(v.integer({ min: 1, max: 100 }), 20),
        offset: v.withDefault(v.integer({ min: 0 }), 0),
        tags: v.optional(v.csv(v.string({ minLength: 1 }))),
      },
      response: WIDGET_QUERY_RESPONSE,
      handler: ({ query, log }) => {
        const wanted = query.tags;
        const filtered = WIDGETS.filter(
          (widget) =>
            (query.status === undefined || widget.status === query.status) &&
            (wanted === undefined ||
              wanted.every((tag) => widget.tags.indexOf(tag) >= 0))
        );
        const page = filtered.slice(query.offset, query.offset + query.limit);
        log.debug("listed widgets", {
          matched: filtered.length,
          returned: page.length,
        });
        return queryResult(page, {
          totalPagedResults: filtered.length,
          remainingPagedResults: Math.max(
            filtered.length - (query.offset + page.length),
            0
          ),
        });
      },
    }),

    route({
      method: "read",
      path: "/{widgetId}",
      params: widgetIdParams,
      query: {
        expand: v.optional(v.csv(v.enumOf(["owner", "history"]))),
      },
      response: WIDGET_RESPONSE,
      handler: ({ params, query }) => {
        const widget = requireWidget(params.widgetId);
        const expand = query.expand ?? [];
        return {
          ...widget,
          ...(expand.indexOf("owner") < 0
            ? {}
            : { ownerDetail: { id: widget.owner, team: widget.owner } }),
          ...(expand.indexOf("history") < 0
            ? {}
            : { history: [{ at: "2026-01-01T00:00:00Z", event: "created" }] }),
        };
      },
    }),

    route({
      method: "create",
      path: "/",
      body: widgetBody,
      response: WIDGET_RESPONSE,
      handler: ({ body, newResourceId, context, log }) => {
        const created = {
          // A bare `PUT .../{id}` arrives here as a create carrying the id.
          // No `padStart`: the IDM lib set stops at ES2016 (see tsconfig.json).
          _id:
            newResourceId ??
            "w-" + ("000" + String(WIDGETS.length + 1)).slice(-4),
          ...body,
          metadata: body.metadata ?? {},
          owner: "unassigned",
        };
        audit(log, context, {
          action: "widget.created",
          subject: widgetKey(created._id),
          fields: { status: created.status },
        });
        return created;
      },
    }),

    route({
      method: "update",
      path: "/{widgetId}",
      params: widgetIdParams,
      body: widgetBody,
      response: WIDGET_RESPONSE,
      handler: ({ params, body }) => {
        const widget = requireWidget(params.widgetId);
        return {
          ...widget,
          ...body,
          metadata: body.metadata ?? {},
        };
      },
    }),

    route({
      method: "patch",
      path: "/{widgetId}",
      params: widgetIdParams,
      patches: v.patchOperations({ minItems: 1, maxItems: 20 }),
      response: WIDGET_RESPONSE,
      handler: ({ params, patchOperations }) => {
        const widget = requireWidget(params.widgetId);
        return { ...widget, _patchOperations: patchOperations.length };
      },
    }),

    route({
      method: "delete",
      path: "/{widgetId}",
      params: widgetIdParams,
      response: WIDGET_RESPONSE,
      handler: ({ params }) => requireWidget(params.widgetId),
    }),

    route({
      method: "action",
      action: "retire",
      path: "/{widgetId}",
      scopes: [WRITE_SCOPE],
      params: widgetIdParams,
      body: v.object({ reason: v.string({ minLength: 1, maxLength: 200 }) }),
      response: WIDGET_RESPONSE,
      handler: ({ params, body, context, log }) => {
        const widget = requireWidget(params.widgetId);
        if (widget.status === "retired") {
          throw conflict("Widget " + widget._id + " is already retired");
        }
        audit(log, context, {
          action: "widget.retired",
          subject: widgetKey(widget._id),
          fields: { reason: body.reason },
        });
        return { ...widget, status: "retired", retiredReason: body.reason };
      },
    }),

    route({
      method: "action",
      action: "bulkImport",
      path: "/",
      scopes: [WRITE_SCOPE],
      body: v.object({
        items: v.list(widgetBody, { minItems: 1, maxItems: 50 }),
      }),
      response: IMPORT_RESPONSE,
      handler: ({ body, log }) => {
        const names = body.items.map((item) => item.name);
        const unique: string[] = [];
        for (const name of names) {
          if (unique.indexOf(name) >= 0) {
            throw badRequest("Duplicate widget name in import", { name });
          }
          unique.push(name);
        }
        log.info("widget.bulkImported", { count: body.items.length });
        return { imported: body.items.length, names: unique };
      },
    }),
  ],
});
