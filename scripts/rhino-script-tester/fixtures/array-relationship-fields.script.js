// Probe: openidm.read field selectors on a has-many relationship from a
// next-gen journey. Creates two throwaway zzemu roles, one zzemu user holding
// both and one holding none, reads each with every selector, then deletes all
// four. Captured on the sandbox 2026-11-08 (docs/api/12).
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}
function attempt(f) { try { return f(); } catch (e) { return "err:" + String(e); } }
var tag = "zzemu-" + String(utils.crypto.randomUUID()).substring(0, 8);
var r1 = tag + "-role-b", r2 = tag + "-role-a", u = String(utils.crypto.randomUUID()), uname = tag + "-user", u2 = String(utils.crypto.randomUUID()), uname2 = tag + "-user2";
var out = { ok: true, feature: "array-relationship-fields", tag: tag, created: [], reads: {}, cleanup: {} };
try {
  out.created.push(attempt(function () { return openidm.create("managed/alpha_role", r1, { name: r1, description: "zzemu probe" })._id; }));
  out.created.push(attempt(function () { return openidm.create("managed/alpha_role", r2, { name: r2, description: "zzemu probe" })._id; }));
  out.created.push(attempt(function () {
    return openidm.create("managed/alpha_user", u, { userName: uname, givenName: "Zz", sn: "Emu", mail: uname + "@example.com",
      roles: [{ _ref: "managed/alpha_role/" + r1 }, { _ref: "managed/alpha_role/" + r2 }] })._id;
  }));
  out.created.push(attempt(function () {
    return openidm.create("managed/alpha_user", u2, { userName: uname2, givenName: "Zz", sn: "Emu", mail: uname2 + "@example.com" })._id;
  }));
  var selectors = [["roles"], ["roles/*"], ["roles/_id"], ["roles/name"], ["roles/*", "userName"], ["*", "roles/*"], ["*", "roles/name"],
    ["roles/_ref"], ["roles/_refProperties"], ["*"], ["roles/*/name/x"], ["roles/*", "roles/_id"], ["roles", "roles/*"], ["roles/*/_refProperties"], ["roles/*/_id"]];
  var empty = [["roles"], ["roles/*"], ["roles/*/name"], ["roles/_id"], ["*", "roles/*"]];
  for (var i = 0; i < selectors.length; i++) {
    (function (sel) {
      out.reads[sel.join("|")] = attempt(function () {
        var rec = openidm.read("managed/alpha_user/" + u, null, sel);
        var roles = rec === null ? undefined : rec.roles;
        return {
          keys: rec === null ? null : Object.keys(rec).sort().join(","),
          rolesType: typeof roles, rolesIsArray: Array.isArray(roles),
          roles: JSON.stringify(roles === undefined ? "<absent>" : roles),
          len: attempt(function () { return roles.length; }), size: attempt(function () { return roles.size(); }),
          first: attempt(function () { return typeof roles[0]; }), map: attempt(function () { return typeof roles.map; })
        };
      });
    })(selectors[i]);
  }
  out.empty = {};
  for (var j = 0; j < empty.length; j++) {
    (function (sel) {
      out.empty[sel.join("|")] = attempt(function () {
        var rec = openidm.read("managed/alpha_user/" + u2, null, sel);
        var roles = rec === null ? undefined : rec.roles;
        return { has: rec !== null && Object.prototype.hasOwnProperty.call(rec, "roles"), rolesType: typeof roles,
          roles: JSON.stringify(roles === undefined ? "<absent>" : roles), size: attempt(function () { return roles.size(); }) };
      });
    })(empty[j]);
  }
} finally {
  out.cleanup.user = attempt(function () { openidm.delete("managed/alpha_user/" + u, null); return "deleted"; });
  out.cleanup.user2 = attempt(function () { openidm.delete("managed/alpha_user/" + u2, null); return "deleted"; });
  out.cleanup.r1 = attempt(function () { openidm.delete("managed/alpha_role/" + r1, null); return "deleted"; });
  out.cleanup.r2 = attempt(function () { openidm.delete("managed/alpha_role/" + r2, null); return "deleted"; });
}
emit(out);
