// Demo endpoint: multi-segment routing over the SAME shared modules as
// `aicdemo-a1-claude-widgets`.
//
// `/widget/{widgetId}/summary` deliberately puts the path parameter in the
// middle of three segments, and `/daily/{date}` deliberately mixes a literal
// with a capture — `request.resourcePath` carries the whole sub-path verbatim
// (`"widget/w-abcd/summary"`), so both are ordinary pattern matches.
//
// User file — seeded once, yours to change.

import {
  badRequest,
  defineEndpoint,
  notFound,
  route,
  v,
} from "../../framework/index.ts";
import { audit } from "../shared/audit.ts";
import { WIDGETS, findWidget } from "../shared/fixtures.ts";
import { widgetId, widgetKey } from "../shared/widget-key.ts";

const GROUPINGS = ["day", "status"] as const;

export default defineEndpoint({
  name: "aicdemo-a1-claude-reports",
  summary: "Widget reports (demo)",
  description:
    "Reads its widget vocabulary from the same shared module as the widgets " +
    "endpoint, so an id that is valid in one is valid in the other.",
  headers: { "x-request-id": v.optional(v.uuid()) },
  routes: [
    route({
      method: "read",
      path: "/daily/{date}",
      summary: "One day's widget activity",
      params: { date: v.isoDate("Report day, YYYY-MM-DD.") },
      handler: ({ params, context, log }) => {
        audit(log, context, {
          action: "report.daily.read",
          fields: { date: params.date },
        });
        return {
          _id: "daily/" + params.date,
          date: params.date,
          created: 2,
          retired: 1,
          active: WIDGETS.filter((widget) => widget.status === "active").length,
        };
      },
    }),

    route({
      method: "read",
      path: "/widget/{widgetId}/summary",
      summary: "Lifetime summary for one widget",
      params: { widgetId: widgetId() },
      handler: ({ params, log }) => {
        const widget = findWidget(params.widgetId);
        if (widget === undefined) {
          throw notFound("No widget " + params.widgetId, {
            key: widgetKey(params.widgetId),
          });
        }
        log.debug("summarised widget", { key: widgetKey(widget._id) });
        return {
          _id: widgetKey(widget._id) + "/summary",
          widgetId: widget._id,
          status: widget.status,
          tagCount: widget.tags.length,
          events: 3,
        };
      },
    }),

    route({
      method: "query",
      path: "/",
      summary: "Aggregate widget counts over a date range",
      query: {
        from: v.isoDate("Inclusive start date."),
        to: v.isoDate("Inclusive end date."),
        groupBy: v.withDefault(v.enumOf(GROUPINGS), "day"),
      },
      handler: ({ query, log }) => {
        if (query.from > query.to) {
          // Lexicographic comparison is exact for YYYY-MM-DD.
          // No backticks: IDM HTML-escapes every string in a thrown error.
          throw badRequest("from must not be after to", {
            from: query.from,
            to: query.to,
          });
        }
        log.debug("aggregating", { groupBy: query.groupBy });
        const rows =
          query.groupBy === "status"
            ? groupByStatus()
            : [
                { _id: query.from, bucket: query.from, count: 2 },
                { _id: query.to, bucket: query.to, count: 1 },
              ];
        return {
          result: rows,
          resultCount: rows.length,
          pagedResultsCookie: null,
          totalPagedResults: rows.length,
          totalPagedResultsPolicy: "EXACT",
        };
      },
    }),
  ],
});

function groupByStatus(): { _id: string; bucket: string; count: number }[] {
  const counts: Record<string, number> = {};
  for (const widget of WIDGETS) {
    counts[widget.status] = (counts[widget.status] ?? 0) + 1;
  }
  return Object.keys(counts).map((status) => ({
    _id: status,
    bucket: status,
    count: counts[status] as number,
  }));
}
