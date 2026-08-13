// In-memory demo data.
//
// The demo endpoints exist to exercise the FRAMEWORK — routing, validation,
// logging, error mapping, OpenAPI — not to be a datastore. Nothing here is
// persisted; replace it with `openidm.*` calls in a real endpoint.
//
// User file — seeded once, yours to change.

import type { WidgetStatus } from "./widget-key.ts";

export interface Widget {
  _id: string;
  name: string;
  status: WidgetStatus;
  tags: string[];
  owner: string;
  metadata: Record<string, string>;
}

export const WIDGETS: Widget[] = [
  {
    _id: "w-abcd",
    name: "Left-handed sprocket",
    status: "active",
    tags: ["mechanical", "stock"],
    owner: "ops",
    metadata: { line: "A" },
  },
  {
    _id: "w-beta01",
    name: "Reference gasket",
    status: "draft",
    tags: ["prototype"],
    owner: "design",
    metadata: {},
  },
  {
    _id: "w-zz9plural",
    name: "Discontinued flange",
    status: "retired",
    tags: ["mechanical"],
    owner: "ops",
    metadata: { line: "B" },
  },
];

export function findWidget(id: string): Widget | undefined {
  for (const widget of WIDGETS) {
    if (widget._id === id) {
      return widget;
    }
  }
  return undefined;
}
