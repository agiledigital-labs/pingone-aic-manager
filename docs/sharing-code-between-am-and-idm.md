# Sharing code between AM and IDM

AM and IDM scripts run in different environments. AM can load realm-scoped AM
library scripts, but IDM cannot import a tenant-authored module. When a small
piece of code must be used from both environments, an IDM custom endpoint can
provide a shared internal API.

```text
                         ┌─ IDM scripts
IDM custom endpoint  ◀───┼─ AM library wrapper ◀── AM scripts
                         └─ Direct endpoint tests
```

This is a service boundary rather than a shared module: calls are synchronous
IDM operations with serialised request and response values.

## When this pattern fits

Use it for a compact library with a small public surface, especially when the
operations are already expressed in terms of plain objects, arrays, strings,
numbers, and booleans.

Avoid it for small helpers called repeatedly in a loop or on a latency-sensitive
journey path. Each call from AM crosses into IDM. Prefer one coarse-grained
operation that completes the whole task inside the endpoint.

Also account for scope: an IDM endpoint is tenant-global, while its AM
compatibility library is realm-scoped. Review the endpoint's access policy and
do not assume that a route is internal merely because its first consumer is
another script.

## Recommended shape

Keep the implementation and its internal helper calls in the endpoint. Expose
only allowlisted named actions:

```javascript
function normaliseAddress(address) {
  return {
    suburb: String(address.suburb || "")
      .trim()
      .toUpperCase(),
    postcode: String(address.postcode || "").trim(),
  };
}

function dispatchAction(actionName, content) {
  switch (actionName) {
    case "normaliseAddress":
      return normaliseAddress(content.address || {});
    default:
      throw new Error("Unsupported shared-tools action: " + actionName);
  }
}

(function () {
  if (request.method !== "action") {
    throw new Error("shared-tools supports action requests only");
  }

  return {
    result: dispatchAction(request.action, request.content || {}),
  };
})();
```

IDM scripts call the endpoint directly:

```javascript
var response = openidm.action("endpoint/shared-tools", "normaliseAddress", {
  address: address,
});
var normalisedAddress = response.result;
```

If AM callers already expect a library function, keep that interface with a
thin realm-scoped AM library:

```javascript
function callSharedTools(actionName, content) {
  var response = openidm.action("endpoint/shared-tools", actionName, content);

  if (!response || !Object.prototype.hasOwnProperty.call(response, "result")) {
    throw new Error("endpoint/shared-tools returned an invalid response");
  }

  return response.result;
}

exports.normaliseAddress = function (address) {
  return callSharedTools("normaliseAddress", { address: address });
};
```

The `{result: ...}` response envelope is deliberate. A custom endpoint returns a
CREST resource object; the envelope carries object, primitive, and `null`
operation results consistently. The wrapper removes the transport detail from
existing AM callers.

## Design rules

- Make action names, payloads, response envelopes, and errors a stable internal
  API.
- Allowlist actions explicitly; do not use a request value to select an
  arbitrary function.
- Validate required input at the boundary and return only values that serialise
  cleanly.
- Let composite actions call local helper functions. Do not recursively invoke
  the endpoint for work already in the same source file.
- Fail normally for business-critical operations. Fail open only for explicitly
  non-critical work such as best-effort telemetry.
- Keep the AM wrapper small. Business logic in the wrapper recreates the
  duplication this pattern is intended to remove.
- Avoid logging request content or the full endpoint `context`; either can
  contain sensitive data.

## Testing

Test the endpoint as the source of truth by invoking:

```text
POST /openidm/endpoint/shared-tools?_action=normaliseAddress
```

Cover every public action, including invalid actions and any primitive or
`null` return values. Then use a smaller contract suite through the AM caller to
confirm that the wrapper passes arguments, unwraps the response, and preserves
the previous function interface.

Measure both a warmed direct endpoint call and the complete AM journey path
before adopting the pattern in latency-sensitive code.

## Working with AIC manager

Custom endpoints appear under `workspace/<tenant>/idm/endpoint/`; AM libraries
appear under `workspace/<tenant>/am/<realm>/lib/`. Once the resources exist in
AIC, pull and manage them with their exact references:

```bash
aic script pull endpoint/shared-tools
aic script pull alpha/lib-shared-tools

aic script status endpoint/shared-tools
aic script status alpha/lib-shared-tools
```

Use the normal `aic script` workflow for changes so its last-synced snapshots
continue to provide content-based conflict detection.

See [IDM custom endpoints](api/11-idm-endpoints.md) for the verified endpoint
configuration, invocation methods, runtime bindings, and JavaScript
restrictions.
