/**
 * One layout inventory and bag-key collision policy for Node validation and
 * Rhino. mockPreamble embeds this self-contained factory's JavaScript source;
 * keep its body AM-safe, with no imports or references to module-scope values.
 * Documented guard-only names do not activate getters or write layouts.
 */
type IdentityFamily = [
  RegExp,
  string,
  "single" | "multi",
  "integer" | "date" | null,
];
type IdentityLayout = {
  field: string;
  none: "remove" | "violation";
  one: "scalar";
  many: "array";
  syntax: null;
};

export function createIdentityPolicy() {
  const families: IdentityFamily[] = [
    [/^fr-attr-str([1-5])$/, "frUnindexedString", "single", null],
    [/^fr-attr-istr([1-9]|1[0-9]|20)$/, "frIndexedString", "single", null],
    [/^fr-attr-multi([1-5])$/, "frUnindexedMultivalued", "multi", null],
    [/^fr-attr-imulti([1-5])$/, "frIndexedMultivalued", "multi", null],
    [/^fr-attr-int([1-5])$/, "frUnindexedInteger", "single", "integer"],
    [/^fr-attr-iint([1-5])$/, "frIndexedInteger", "single", "integer"],
    [/^fr-attr-date([1-5])$/, "frUnindexedDate", "single", "date"],
    [/^fr-attr-idate([1-5])$/, "frIndexedDate", "single", "date"],
  ];
  // Standard LDAP attributes are multi-valued in DS: one value is a string in
  // IDM, several are an array, none removes it. sn is required, so none is a
  // violation; mail, givenName and telephoneNumber are not.
  const standard: Record<string, IdentityLayout> = {
    givenName: {
      field: "givenName",
      none: "remove",
      one: "scalar",
      many: "array",
      syntax: null,
    },
    sn: {
      field: "sn",
      none: "violation",
      one: "scalar",
      many: "array",
      syntax: null,
    },
    mail: {
      field: "mail",
      none: "remove",
      one: "scalar",
      many: "array",
      syntax: null,
    },
    telephoneNumber: {
      field: "telephoneNumber",
      none: "remove",
      one: "scalar",
      many: "array",
      syntax: null,
    },
  };
  // Documented ordinary IDM targets whose AM write layout is not measured here.
  // Guard-only inventory: these names do not enable getters or writes.
  const documentedFields = [
    "userName",
    "cn",
    "accountStatus",
    "displayName",
    "description",
    "password",
    "postalAddress",
    "city",
    "stateProvince",
    "postalCode",
    "country",
    "aliasList",
    "applications",
    "ownerOfApp",
    "assignedDashboard",
    "assignments",
    "consentedMappings",
    "reports",
    "manager",
    "passwordLastChangedTime",
    "passwordExpirationTime",
    "groups",
    "roles",
    "kbaInfo",
    "preferences",
    "profileImage",
    "adminOfOrg",
    "ownerOfOrg",
    "memberOfOrg",
    "memberOfOrgIDs",
    "taskPrincipals",
    "deviceProfiles",
    "devicePrintProfiles",
    "webauthnDeviceProfiles",
    "oathDeviceProfiles",
    "pushDeviceProfiles",
    "fr-idm-custom-attrs",
  ];
  function collisionReason(property: string): string | null {
    if (property.charAt(0) === "_") {
      return "reserved record metadata";
    }
    if (
      documentedFields.indexOf(property) !== -1 ||
      Object.keys(standard).some(function (name) {
        const layout = standard[name];
        return layout !== undefined && layout.field === property;
      }) ||
      families.some(function (family) {
        // Keep the measured family's numeric bounds, replacing only its AM prefix.
        const pattern = family[0].source.replace(
          /^\^fr-attr-[^(]+/,
          "^" + family[1],
        );
        return new RegExp(pattern).test(property);
      })
    ) {
      return "another AM attribute's IDM field";
    }
    return null;
  }
  // Property assignments survive host transforms without Rhino-incompatible shorthand.
  const policy = {} as {
    families: IdentityFamily[];
    standard: Record<string, IdentityLayout>;
    collisionReason: (property: string) => string | null;
  };
  policy.families = families;
  policy.standard = standard;
  policy.collisionReason = collisionReason;
  return policy;
}

export const identityPolicy = createIdentityPolicy();
