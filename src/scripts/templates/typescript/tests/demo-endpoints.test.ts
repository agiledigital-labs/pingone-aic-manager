// The two demo endpoints, driven through `dispatch` exactly as IDM drives the
// bundle. Delete this alongside `src/endpoints/example-*` when you replace the
// demo with your own endpoints.
//
// User file — seeded once, yours to change.

import assert from "node:assert/strict";
import { test } from "node:test";

import reports from "../src/endpoints/example-reports.ts";
import widgets from "../src/endpoints/example-widgets.ts";
import { callContext, crestErrorFrom, crestRequest } from "./harness.ts";

const WRITE = ["example:widgets:write"];

test("query returns a CREST query result and honours status + paging", () => {
  const all = widgets.dispatch(crestRequest("query"), callContext()) as {
    result: { _id: string }[];
    totalPagedResults: number;
  };
  assert.equal(all.result.length, 3);
  assert.equal(all.totalPagedResults, 3);

  const active = widgets.dispatch(
    crestRequest("query", { query: { status: "active" } }),
    callContext()
  ) as { result: { _id: string }[] };
  assert.deepEqual(
    active.result.map((widget) => widget._id),
    ["w-abcd"]
  );

  const paged = widgets.dispatch(
    crestRequest("query", { query: { limit: "1", offset: "1" } }),
    callContext()
  ) as { result: { _id: string }[]; remainingPagedResults: number };
  assert.deepEqual(
    paged.result.map((widget) => widget._id),
    ["w-beta01"]
  );
  assert.equal(paged.remainingPagedResults, 1);
});

test("tags are a comma-separated AND filter", () => {
  const result = widgets.dispatch(
    crestRequest("query", { query: { tags: "mechanical,stock" } }),
    callContext()
  ) as { result: { _id: string }[] };
  assert.deepEqual(
    result.result.map((widget) => widget._id),
    ["w-abcd"]
  );
});

test("limit above its declared maximum is a 400", () => {
  assert.equal(
    crestErrorFrom(() =>
      widgets.dispatch(
        crestRequest("query", { query: { limit: "1000" } }),
        callContext()
      )
    ).code,
    400
  );
});

test("read expands only what was asked for", () => {
  const plain = widgets.dispatch(
    crestRequest("read", { resourcePath: "w-abcd" }),
    callContext()
  ) as Record<string, unknown>;
  assert.equal(plain["ownerDetail"], undefined);

  const expanded = widgets.dispatch(
    crestRequest("read", {
      resourcePath: "w-abcd",
      query: { expand: "owner" },
    }),
    callContext()
  ) as Record<string, unknown>;
  assert.deepEqual(expanded["ownerDetail"], { id: "ops", team: "ops" });
  assert.equal(expanded["history"], undefined);
});

test("a widget id that fails the shared pattern is a 400", () => {
  assert.equal(
    crestErrorFrom(() =>
      widgets.dispatch(
        crestRequest("read", { resourcePath: "nope" }),
        callContext()
      )
    ).code,
    400
  );
});

test("a well-formed id that does not exist is a 404", () => {
  assert.equal(
    crestErrorFrom(() =>
      widgets.dispatch(
        crestRequest("read", { resourcePath: "w-zzzz" }),
        callContext()
      )
    ).code,
    404
  );
});

test("create validates the whole body", () => {
  const created = widgets.dispatch(
    crestRequest("create", {
      content: { name: "New", status: "draft", tags: ["a"] },
    }),
    callContext()
  ) as { name: string; metadata: Record<string, string> };
  assert.equal(created.name, "New");
  assert.deepEqual(created.metadata, {});

  const fault = crestErrorFrom(() =>
    widgets.dispatch(
      crestRequest("create", {
        content: { name: "", status: "nope", tags: "x" },
      }),
      callContext()
    )
  );
  assert.equal(fault.code, 400);
  assert.equal((fault.detail as { issues: unknown[] }).issues.length, 3);
});

test("retire needs the write scope and refuses an already-retired widget", () => {
  assert.equal(
    crestErrorFrom(() =>
      widgets.dispatch(
        crestRequest("action", {
          resourcePath: "w-abcd",
          action: "retire",
          content: { reason: "obsolete" },
        }),
        callContext()
      )
    ).code,
    403
  );

  const retired = widgets.dispatch(
    crestRequest("action", {
      resourcePath: "w-abcd",
      action: "retire",
      content: { reason: "obsolete" },
    }),
    callContext({ scopes: WRITE })
  ) as { status: string; retiredReason: string };
  assert.equal(retired.status, "retired");
  assert.equal(retired.retiredReason, "obsolete");

  assert.equal(
    crestErrorFrom(() =>
      widgets.dispatch(
        crestRequest("action", {
          resourcePath: "w-zz9plural",
          action: "retire",
          content: { reason: "already gone" },
        }),
        callContext({ scopes: WRITE })
      )
    ).code,
    409
  );
});

test("bulkImport bounds the batch and rejects duplicate names", () => {
  const item = { name: "A", status: "draft", tags: [] };
  const imported = widgets.dispatch(
    crestRequest("action", {
      action: "bulkImport",
      content: { items: [item, { ...item, name: "B" }] },
    }),
    callContext({ scopes: WRITE })
  ) as { imported: number };
  assert.equal(imported.imported, 2);

  assert.equal(
    crestErrorFrom(() =>
      widgets.dispatch(
        crestRequest("action", {
          action: "bulkImport",
          content: { items: [item, item] },
        }),
        callContext({ scopes: WRITE })
      )
    ).code,
    400
  );

  const tooMany = Array.from({ length: 51 }, (_unused, index) => ({
    ...item,
    name: "n" + index,
  }));
  assert.equal(
    crestErrorFrom(() =>
      widgets.dispatch(
        crestRequest("action", {
          action: "bulkImport",
          content: { items: tooMany },
        }),
        callContext({ scopes: WRITE })
      )
    ).code,
    400
  );
});

test("reports routes a literal-then-capture path", () => {
  const daily = reports.dispatch(
    crestRequest("read", { resourcePath: "daily/2026-08-13" }),
    callContext()
  ) as { date: string };
  assert.equal(daily.date, "2026-08-13");

  assert.equal(
    crestErrorFrom(() =>
      reports.dispatch(
        crestRequest("read", { resourcePath: "daily/13-08-2026" }),
        callContext()
      )
    ).code,
    400
  );
});

test("reports routes a capture in the middle of three segments", () => {
  const summary = reports.dispatch(
    crestRequest("read", { resourcePath: "widget/w-abcd/summary" }),
    callContext()
  ) as { widgetId: string; _id: string };
  assert.equal(summary.widgetId, "w-abcd");
  assert.equal(summary._id, "widget/w-abcd/summary");
});

test("reports rejects an id the SHARED widget module considers invalid", () => {
  // Same validator as the widgets endpoint — one definition, two endpoints.
  assert.equal(
    crestErrorFrom(() =>
      reports.dispatch(
        crestRequest("read", { resourcePath: "widget/NOPE/summary" }),
        callContext()
      )
    ).code,
    400
  );
});

test("reports groups by status when asked, by day by default", () => {
  const byDay = reports.dispatch(
    crestRequest("query", { query: { from: "2026-08-01", to: "2026-08-02" } }),
    callContext()
  ) as { result: { bucket: string }[] };
  assert.deepEqual(
    byDay.result.map((row) => row.bucket),
    ["2026-08-01", "2026-08-02"]
  );

  const byStatus = reports.dispatch(
    crestRequest("query", {
      query: { from: "2026-08-01", to: "2026-08-02", groupBy: "status" },
    }),
    callContext()
  ) as { result: { bucket: string }[] };
  assert.deepEqual(byStatus.result.map((row) => row.bucket).sort(), [
    "active",
    "draft",
    "retired",
  ]);
});

test("reports refuses an inverted date range and a missing one", () => {
  assert.equal(
    crestErrorFrom(() =>
      reports.dispatch(
        crestRequest("query", {
          query: { from: "2026-08-09", to: "2026-08-01" },
        }),
        callContext()
      )
    ).code,
    400
  );
  assert.equal(
    crestErrorFrom(() =>
      reports.dispatch(crestRequest("query", { query: {} }), callContext())
    ).code,
    400
  );
});

test("both endpoints reject an unknown sub-path with a 404", () => {
  for (const endpoint of [widgets, reports]) {
    assert.equal(
      crestErrorFrom(() =>
        endpoint.dispatch(
          crestRequest("read", { resourcePath: "no/such/route/here" }),
          callContext()
        )
      ).code,
      404
    );
  }
});
