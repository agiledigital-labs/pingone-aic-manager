/*
 * User-token endpoint proof of concept.
 *
 * Deploy as: /openidm/endpoint/user-token-poc/whoami
 *
 * The IDM rsFilter has already validated the AM bearer token and populated
 * context.security before this script is invoked. Do not parse, decode, or
 * introspect Authorization here.
 */
(function () {
  function keysOf(value) {
    if (!value) {
      return [];
    }
    try {
      return Object.keys(value);
    } catch (error) {
      return ["<not-enumerable>"];
    }
  }

  function describe(value) {
    if (typeof value === "undefined") {
      return { present: false };
    }
    if (value === null) {
      return { present: true, type: "object", value: null };
    }
    return {
      present: true,
      type: typeof value,
      value: String(value)
    };
  }

  if (request.method !== "read") {
    throw { code: 405, message: "GET is the only supported method" };
  }

  if (request.resourcePath !== "whoami") {
    throw { code: 404, message: "Unknown resource" };
  }

  var security = context && context.security;
  var authorization = security && security.authorization;
  var roles = authorization && authorization.roles;

  // The matching config/access rule stops unauthenticated callers before they
  // reach this script. Keep this check as defence in depth and to make a bad
  // access-rule deployment fail closed.
  if (!security || !security.authenticationId || !authorization || !roles) {
    throw { code: 401, message: "A valid tenant bearer token is required" };
  }

  var grantedRole = null;
  for (var i = 0; i < endpointConfig.allowedRoles.length; i += 1) {
    if (roles.indexOf(endpointConfig.allowedRoles[i]) !== -1) {
      grantedRole = endpointConfig.allowedRoles[i];
      break;
    }
  }

  if (!grantedRole) {
    throw { code: 403, message: "Missing required API role" };
  }

  return {
    _id: "whoami",
    subject: security.authenticationId,
    component: authorization.component,
    grantedRole: grantedRole,
    message: "Token validation and role authorization succeeded",
    request: request,
    contextProbe: {
      keys: keysOf(context),
      http: context.http
        ? {
            method: context.http.method,
            path: context.http.path,
            parameters: context.http.parameters,
            headerNames: keysOf(context.http.headers)
          }
        : null,
      security: {
        authenticationId: security.authenticationId,
        authorization: {
          id: authorization.id,
          component: authorization.component,
          roles: roles
        }
      },
      oauth2: context.oauth2
        ? {
            keys: keysOf(context.oauth2),
            rawInfoKeys: keysOf(context.oauth2.rawInfo),
            accessTokenKeys: keysOf(context.oauth2.accessToken),
            accessTokenInfoKeys: keysOf(
              context.oauth2.accessToken && context.oauth2.accessToken.info
            ),
            scope: describe(context.oauth2.scope),
            scopes: describe(context.oauth2.scopes),
            rawInfoScope: describe(
              context.oauth2.rawInfo && context.oauth2.rawInfo.scope
            ),
            accessTokenScopes: describe(
              context.oauth2.accessToken && context.oauth2.accessToken.scopes
            ),
            accessTokenInfoScope: describe(
              context.oauth2.accessToken &&
                context.oauth2.accessToken.info &&
                context.oauth2.accessToken.info.scope
            )
          }
        : null
    }
  };
})();
