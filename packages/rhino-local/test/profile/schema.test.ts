import { describe, expect, it } from "vitest";
import { normaliseManagedConfig, ProfileShapeError } from "../../src/profile/normalise.ts";
import {
  checkRecord,
  knowsType,
  SchemaViolation,
  splitResource,
  unknownTypeError,
} from "../../src/profile/validate.ts";
import type { EnvProfile } from "../../src/profile/types.ts";

const META = { tenant: "sandbox", pulledAt: "2026-09-12T00:00:00.000Z", endpoint: "/openidm/config/managed" };

function wire(objects: unknown[]): unknown {
  return { _id: "managed", objects };
}

describe("normalising /openidm/config/managed", () => {
  it("collapses IDM's nullable spelling to one type plus a flag", () => {
    // ["string","null"] must not become the type "array", nor "string,null".
    const profile = normaliseManagedConfig(
      wire([{ name: "alpha_user", schema: { properties: { mail: { type: ["string", "null"] } } } }]),
      META
    );
    expect(profile.objects.alpha_user?.properties.mail).toEqual({
      type: "string",
      nullable: true,
    });
  });

  it("keeps a non-nullable scalar free of the flag", () => {
    const profile = normaliseManagedConfig(
      wire([{ name: "alpha_user", schema: { properties: { sn: { type: "string" } } } }]),
      META
    );
    expect(profile.objects.alpha_user?.properties.sn).toEqual({ type: "string" });
  });

  it("registers an object that carries no schema block at all", () => {
    // Existence is what separates an AIC null from a fixture typo. An object
    // described by nothing must still be known to exist.
    const profile = normaliseManagedConfig(wire([{ name: "alpha_lock" }]), META);
    expect(Object.hasOwn(profile.objects, "alpha_lock")).toBe(true);
    expect(profile.objects.alpha_lock?.properties).toEqual({});
  });

  it("flattens relationship targets out of resourceCollection", () => {
    const profile = normaliseManagedConfig(
      wire([
        {
          name: "alpha_user",
          schema: {
            properties: {
              manager: {
                type: "relationship",
                resourceCollection: [{ path: "managed/alpha_user", label: "User" }],
              },
            },
          },
        },
      ]),
      META
    );
    expect(profile.objects.alpha_user?.properties.manager?.resourceCollection).toEqual([
      "managed/alpha_user",
    ]);
  });

  it("carries an enum through an array's items, not just a scalar", () => {
    const profile = normaliseManagedConfig(
      wire([
        {
          name: "alpha_user",
          schema: {
            properties: {
              tags: { type: "array", items: { type: "string", enum: ["a", "b"] } },
            },
          },
        },
      ]),
      META
    );
    expect(profile.objects.alpha_user?.properties.tags?.items?.enum).toEqual(["a", "b"]);
  });

  it("drops lifecycle hook source rather than storing client code", () => {
    const profile = normaliseManagedConfig(
      wire([
        {
          name: "alpha_user",
          schema: { properties: {} },
          onCreate: { type: "text/javascript", source: "SECRET_BUSINESS_LOGIC" },
        },
      ]),
      META
    );
    expect(JSON.stringify(profile)).not.toContain("SECRET_BUSINESS_LOGIC");
  });

  it("keys each object by name and carries that name into error text", () => {
    // `name` is redundant with the key until an error message uses it, and
    // then a blank one yields "managed/ requires ...". Pin both.
    const built = normaliseManagedConfig(
      wire([{ name: "alpha_lock", schema: { properties: {}, required: ["holder"] } }]),
      META
    );
    expect(built.objects.alpha_lock?.name).toBe("alpha_lock");
    expect(() => checkRecord(built, "managed/alpha_lock/x", {}, "create")).toThrow(
      /managed\/alpha_lock requires "holder"/
    );
  });

  it("refuses a document with no objects array", () => {
    expect(() => normaliseManagedConfig({ _id: "managed" }, META)).toThrow(ProfileShapeError);
  });
});

const profile: EnvProfile = {
  tenant: "sandbox",
  pulledAt: META.pulledAt,
  sources: [META.endpoint],
  objects: {
    alpha_user: {
      name: "alpha_user",
      properties: {
        _id: { type: "string" },
        userName: { type: "string" },
        mail: { type: "string", nullable: true },
        accountStatus: { type: "string", enum: ["active", "inactive"] },
        tags: { type: "array", items: { type: "string", enum: ["a", "b"] } },
      },
      required: ["userName"],
    },
    alpha_role: { name: "alpha_role", properties: {}, required: [] },
  },
};

describe("resource resolution", () => {
  it("splits a managed resource into type and id", () => {
    expect(splitResource("managed/alpha_user/alice")).toEqual({
      type: "managed/alpha_user",
      id: "alice",
    });
  });

  it("treats a bare collection path as a type with no id", () => {
    expect(splitResource("managed/alpha_user")).toEqual({ type: "managed/alpha_user" });
  });

  it("knows a declared type even when the harness has seeded no records for it", () => {
    // This is the distinction the whole profile exists to make: a real type
    // with no record is an AIC `null`, not a fixture bug.
    expect(knowsType(profile, "managed/alpha_role/anything")).toBe(true);
  });

  it("does not know an undeclared type", () => {
    expect(knowsType(profile, "managed/zzz_nope/x")).toBe(false);
  });

  it("names a near miss in the unknown-type error", () => {
    const error = unknownTypeError(profile, "managed/alpha_users/x");
    expect(error.message).toContain("managed/alpha_user");
    expect(error.kind).toBe("unknown-type");
  });
});

describe("strict record checking", () => {
  it("accepts a record whose properties are all declared", () => {
    expect(() =>
      checkRecord(profile, "managed/alpha_user/alice", { _id: "alice", userName: "alice" }, "seed")
    ).not.toThrow();
  });

  it("rejects a misspelled property in a seeded fixture", () => {
    try {
      checkRecord(profile, "managed/alpha_user/alice", { userName: "alice", emai1: "x" }, "seed");
      throw new Error("expected a violation");
    } catch (error) {
      expect(error).toBeInstanceOf(SchemaViolation);
      expect((error as SchemaViolation).kind).toBe("unknown-property");
      expect((error as SchemaViolation).message).toContain("emai1");
    }
  });

  it("allows CREST metadata that no schema declares", () => {
    expect(() =>
      checkRecord(profile, "managed/alpha_user/alice", { userName: "a", _rev: "3", _meta: {} }, "seed")
    ).not.toThrow();
  });

  it("enforces required on create but NOT on a seeded fixture", () => {
    // The discriminating pair. A partial fixture is legitimate; a create that
    // omits a required property is what AIC rejects.
    const partial = { _id: "alice" };
    expect(() => checkRecord(profile, "managed/alpha_user/alice", partial, "seed")).not.toThrow();
    expect(() => checkRecord(profile, "managed/alpha_user/alice", partial, "create")).toThrow(
      /requires "userName"/
    );
  });

  it("rejects an out-of-enum scalar", () => {
    expect(() =>
      checkRecord(profile, "managed/alpha_user/alice", { userName: "a", accountStatus: "banned" }, "seed")
    ).toThrow(/not an allowed value/);
  });

  it("accepts an in-enum scalar", () => {
    expect(() =>
      checkRecord(profile, "managed/alpha_user/alice", { userName: "a", accountStatus: "active" }, "seed")
    ).not.toThrow();
  });

  it("checks enum on array ELEMENTS, not the array itself", () => {
    expect(() =>
      checkRecord(profile, "managed/alpha_user/alice", { userName: "a", tags: ["a", "zzz"] }, "seed")
    ).toThrow(/"zzz" is not an allowed value/);
    expect(() =>
      checkRecord(profile, "managed/alpha_user/alice", { userName: "a", tags: ["a", "b"] }, "seed")
    ).not.toThrow();
  });

  it("tolerates null on a nullable property", () => {
    expect(() =>
      checkRecord(profile, "managed/alpha_user/alice", { userName: "a", mail: null }, "seed")
    ).not.toThrow();
  });

  it("throws unknown-type before it looks at properties", () => {
    expect(() => checkRecord(profile, "managed/zzz/x", { anything: 1 }, "seed")).toThrow(
      /is not a managed object/
    );
  });
});
