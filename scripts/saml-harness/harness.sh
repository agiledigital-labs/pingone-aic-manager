#!/usr/bin/env bash
# scripts/saml-harness/harness.sh
#
# Local Keycloak SAML peer for the forthcoming `aic saml` CLI.
#
# Keycloak is both a SAML IdP (realm aic-idp) and, via identity brokering, a
# SAML SP (realm aic-sp). This script brings a throwaway container up, bootstraps
# those two realms, and can rotate realm signing keys so the SAML descriptor
# grows a second <KeyDescriptor use="signing"> and shrinks back to one.
#
# It never talks to an AIC tenant. Metadata exchange is file/paste: SAML web
# SSO is front-channel, and AIC cannot fetch http://localhost from the cloud.
#
# Admin user/password default to admin/admin. That is a local-dev credential
# for this throwaway container, not a secret.
#
# Usage: scripts/saml-harness/harness.sh <command> [args]
# See `harness.sh help` and docs/saml-test-harness.md.

set -euo pipefail

# The image tag is the current 26.4.x on quay (official getting-started pin).
# The task named `quay.io/keycloak/keycloak:26.4`; that floating tag is not
# published — 26.4.7 is.
IMAGE="${KEYCLOAK_IMAGE:-quay.io/keycloak/keycloak:26.4.7}"
CONTAINER_NAME="${KEYCLOAK_CONTAINER:-aic-saml-keycloak}"
VOLUME_NAME="${KEYCLOAK_VOLUME:-aic-saml-keycloak-data}"
HOST_PORT="${KEYCLOAK_PORT:-18080}"
BIND_ADDR="${KEYCLOAK_BIND:-127.0.0.1}"
ADMIN_USER="${KEYCLOAK_ADMIN_USER:-admin}"
ADMIN_PASSWORD="${KEYCLOAK_ADMIN_PASSWORD:-admin}"
WAIT_SECS="${KEYCLOAK_WAIT_SECS:-180}"

PUBLIC_BASE="http://localhost:${HOST_PORT}"
CURL_BASE="http://${BIND_ADDR}:${HOST_PORT}"

REALM_IDP="${KEYCLOAK_REALM_IDP:-aic-idp}"
REALM_SP="${KEYCLOAK_REALM_SP:-aic-sp}"
IDP_ALIAS="${KEYCLOAK_IDP_ALIAS:-aic}"

TEST_USER="${KEYCLOAK_TEST_USER:-samluser}"
TEST_PASSWORD="${KEYCLOAK_TEST_PASSWORD:-samluser-pass}"
TEST_EMAIL="${KEYCLOAK_TEST_EMAIL:-samluser@example.com}"

# Placeholder AIC-as-SP entity id, used until `register-sp` imports real
# metadata. Reserved placeholder vocabulary from .ai/core.md §3.
PLACEHOLDER_SP_ENTITY="${KEYCLOAK_PLACEHOLDER_SP:-https://sp.example.com}"
PLACEHOLDER_SP_ACS="${KEYCLOAK_PLACEHOLDER_ACS:-https://sp.example.com/acs}"
PLACEHOLDER_IDP_ENTITY="${KEYCLOAK_PLACEHOLDER_IDP:-https://idp.example.com}"
PLACEHOLDER_IDP_SSO="${KEYCLOAK_PLACEHOLDER_SSO:-https://idp.example.com/sso}"

TOKEN=""
KC_CODE=""
KC_BODY=""

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

log() {
  printf '%s\n' "$*" >&2
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

usage() {
  cat <<EOF
Usage: $0 <command> [args]

Lifecycle
  up                 Start Keycloak in dev mode and bootstrap both realms
  down               Stop and remove the container (keeps the volume)
  reset              Wipe the volume and start clean
  status             Container, realms, users, keys, KeyDescriptor counts

Metadata (file/paste; AIC cannot fetch localhost)
  metadata <realm>   Print the realm SAML IdP descriptor
  sp-metadata        Print aic-sp's broker SP descriptor (alias ${IDP_ALIAS})

Certificate rotation (measured against the descriptor, not inferred)
  rotate-add [realm] Add a higher-priority rsa-generated signing key
  rotate-rm  [realm] Remove the lowest-priority rsa-generated signing key
  verify-rotate [realm]
                     Count signing KeyDescriptors, add, count, remove, count

Peer registration (paste AIC metadata in; never a URL fetch)
  register-sp <file> Import an SPSSODescriptor as a SAML client in ${REALM_IDP}
  register-idp <file>
                     Import an IDPSSODescriptor as identity provider in ${REALM_SP}
  wire-loopback      Point ${REALM_SP}'s broker at ${REALM_IDP} (Keycloak-only smoke)

  help               This text

Env (all optional)
  KEYCLOAK_IMAGE KEYCLOAK_CONTAINER KEYCLOAK_VOLUME KEYCLOAK_PORT
  KEYCLOAK_BIND KEYCLOAK_ADMIN_USER KEYCLOAK_ADMIN_PASSWORD
  KEYCLOAK_WAIT_SECS KEYCLOAK_TEST_USER KEYCLOAK_TEST_PASSWORD
  KEYCLOAK_TEST_EMAIL

Defaults: ${IMAGE} on ${BIND_ADDR}:${HOST_PORT}, admin/admin (local-dev).
EOF
}

container_exists() {
  docker inspect "$CONTAINER_NAME" >/dev/null 2>&1
}

container_running() {
  [ "$(docker inspect -f '{{.State.Running}}' "$CONTAINER_NAME" 2>/dev/null || echo false)" = "true" ]
}

wait_ready() {
  local i=0
  local tok
  log "waiting for Keycloak on ${CURL_BASE} (up to ${WAIT_SECS}s)…"
  while [ "$i" -lt "$WAIT_SECS" ]; do
    tok="$(
      curl -sS --max-time 3 -X POST \
        "${CURL_BASE}/realms/master/protocol/openid-connect/token" \
        -d "client_id=admin-cli" \
        -d "username=${ADMIN_USER}" \
        -d "password=${ADMIN_PASSWORD}" \
        -d "grant_type=password" 2>/dev/null \
        | jq -r '.access_token // empty' || true
    )"
    if [ -n "$tok" ]; then
      TOKEN="$tok"
      log "ready after ${i}s"
      return 0
    fi
    if container_exists && ! container_running; then
      docker logs "$CONTAINER_NAME" >&2 || true
      die "container ${CONTAINER_NAME} exited before becoming ready"
    fi
    sleep 2
    i=$((i + 2))
  done
  docker logs "$CONTAINER_NAME" >&2 || true
  die "timed out waiting for Keycloak admin token on ${CURL_BASE}"
}

ensure_token() {
  if [ -n "$TOKEN" ]; then
    return 0
  fi
  container_running || die "container ${CONTAINER_NAME} is not running; try: $0 up"
  TOKEN="$(
    curl -fsS --max-time 5 -X POST \
      "${CURL_BASE}/realms/master/protocol/openid-connect/token" \
      -d "client_id=admin-cli" \
      -d "username=${ADMIN_USER}" \
      -d "password=${ADMIN_PASSWORD}" \
      -d "grant_type=password" | jq -r '.access_token'
  )"
  [ -n "$TOKEN" ] && [ "$TOKEN" != "null" ] || die "failed to obtain admin token"
}

# kc_req METHOD PATH [curl args…]
# Sets KC_CODE and KC_BODY. Does not fail on HTTP errors.
kc_req() {
  local method="$1"
  local path="$2"
  shift 2
  local tmp
  ensure_token
  tmp="$(mktemp)"
  KC_CODE="$(
    curl -sS -o "$tmp" -w '%{http_code}' -X "$method" \
      -H "Authorization: Bearer ${TOKEN}" \
      -H "Content-Type: application/json" \
      "${CURL_BASE}${path}" "$@"
  )"
  KC_BODY="$(cat "$tmp")"
  rm -f "$tmp"
}

kc_expect() {
  local want="$1"
  local what="$2"
  if [ "$KC_CODE" != "$want" ]; then
    die "${what}: expected HTTP ${want}, got ${KC_CODE}: ${KC_BODY}"
  fi
}

realm_exists() {
  local realm="$1"
  kc_req GET "/admin/realms/${realm}"
  [ "$KC_CODE" = "200" ]
}

user_id() {
  local realm="$1"
  local username="$2"
  kc_req GET "/admin/realms/${realm}/users?username=${username}&exact=true"
  kc_expect 200 "list users in ${realm}"
  printf '%s' "$KC_BODY" | jq -r '.[0].id // empty'
}

client_uuid() {
  local realm="$1"
  local client_id="$2"
  kc_req GET "/admin/realms/${realm}/clients?clientId=${client_id}"
  kc_expect 200 "list clients in ${realm}"
  printf '%s' "$KC_BODY" | jq -r '.[0].id // empty'
}

idp_exists() {
  local realm="$1"
  local alias="$2"
  kc_req GET "/admin/realms/${realm}/identity-provider/instances/${alias}"
  [ "$KC_CODE" = "200" ]
}

realm_uuid() {
  local realm="$1"
  kc_req GET "/admin/realms/${realm}"
  kc_expect 200 "read realm ${realm}"
  printf '%s' "$KC_BODY" | jq -r '.id'
}

fetch_descriptor() {
  local realm="$1"
  curl -fsS "${CURL_BASE}/realms/${realm}/protocol/saml/descriptor"
}

# stdin: SAML metadata XML
# stdout: "total signing encryption unspecified"
count_key_descriptors() {
  python3 -c '
import sys
import xml.etree.ElementTree as ET
raw = sys.stdin.read()
root = ET.fromstring(raw)
ns = "urn:oasis:names:tc:SAML:2.0:metadata"
keys = list(root.iter("{%s}KeyDescriptor" % ns))
def n(use):
    return sum(1 for k in keys if k.get("use") == use)
unspec = sum(1 for k in keys if k.get("use") is None)
print("%d %d %d %d" % (len(keys), n("signing"), n("encryption"), unspec))
'
}

pretty_xml() {
  python3 -c '
import sys
import xml.etree.ElementTree as ET
ET.register_namespace("md", "urn:oasis:names:tc:SAML:2.0:metadata")
ET.register_namespace("ds", "http://www.w3.org/2000/09/xmldsig#")
ET.register_namespace("saml", "urn:oasis:names:tc:SAML:2.0:assertion")
raw = sys.stdin.read()
root = ET.fromstring(raw)
ET.indent(root, space="  ")
sys.stdout.write(ET.tostring(root, encoding="unicode"))
sys.stdout.write("\n")
'
}

signing_count() {
  local realm="$1"
  fetch_descriptor "$realm" | count_key_descriptors | awk '{print $2}'
}

cmd_up() {
  need docker
  need curl
  need jq
  need python3

  if container_running; then
    log "container ${CONTAINER_NAME} already running"
  elif container_exists; then
    log "starting existing container ${CONTAINER_NAME}"
    docker start "$CONTAINER_NAME" >/dev/null
  else
    log "starting ${IMAGE} as ${CONTAINER_NAME} on ${BIND_ADDR}:${HOST_PORT}"
    docker run -d \
      --name "$CONTAINER_NAME" \
      --label aic.saml-harness=1 \
      -p "${BIND_ADDR}:${HOST_PORT}:8080" \
      -e "KC_BOOTSTRAP_ADMIN_USERNAME=${ADMIN_USER}" \
      -e "KC_BOOTSTRAP_ADMIN_PASSWORD=${ADMIN_PASSWORD}" \
      -e "KC_HOSTNAME=${PUBLIC_BASE}" \
      -e KC_HTTP_ENABLED=true \
      -v "${VOLUME_NAME}:/opt/keycloak/data" \
      "$IMAGE" \
      start-dev >/dev/null
  fi
  wait_ready
  bootstrap
  cmd_status
}

cmd_down() {
  need docker
  if container_exists; then
    log "removing container ${CONTAINER_NAME} (volume ${VOLUME_NAME} kept)"
    docker rm -f "$CONTAINER_NAME" >/dev/null
  else
    log "container ${CONTAINER_NAME} is not present"
  fi
}

cmd_reset() {
  need docker
  cmd_down
  if docker volume inspect "$VOLUME_NAME" >/dev/null 2>&1; then
    log "removing volume ${VOLUME_NAME}"
    docker volume rm "$VOLUME_NAME" >/dev/null
  fi
  TOKEN=""
  cmd_up
}

create_realm() {
  local realm="$1"
  local display="$2"
  if realm_exists "$realm"; then
    log "realm ${realm} already exists"
    return 0
  fi
  kc_req POST "/admin/realms" --data "$(
    jq -n --arg realm "$realm" --arg display "$display" \
      '{realm: $realm, enabled: true, displayName: $display}'
  )"
  kc_expect 201 "create realm ${realm}"
  log "created realm ${realm}"
}

create_user() {
  local realm="$1"
  local uid
  uid="$(user_id "$realm" "$TEST_USER")"
  if [ -n "$uid" ]; then
    log "user ${TEST_USER} already exists in ${realm}"
    return 0
  fi
  kc_req POST "/admin/realms/${realm}/users" --data "$(
    jq -n \
      --arg username "$TEST_USER" \
      --arg email "$TEST_EMAIL" \
      --arg password "$TEST_PASSWORD" \
      '{
         username: $username,
         enabled: true,
         email: $email,
         emailVerified: true,
         firstName: "SAML",
         lastName: "User",
         credentials: [{type: "password", value: $password, temporary: false}]
       }'
  )"
  kc_expect 201 "create user ${TEST_USER} in ${realm}"
  log "created user ${TEST_USER} in ${realm}"
}

create_placeholder_sp_client() {
  local cid
  cid="$(client_uuid "$REALM_IDP" "$PLACEHOLDER_SP_ENTITY")"
  if [ -n "$cid" ]; then
    log "SAML client ${PLACEHOLDER_SP_ENTITY} already exists in ${REALM_IDP}"
    return 0
  fi
  # saml.client.signature=false on create: Keycloak does not mint a client
  # signing private key (measured). Default create mints one; do not PUT that
  # representation anywhere it could be committed.
  kc_req POST "/admin/realms/${REALM_IDP}/clients" --data "$(
    jq -n \
      --arg clientId "$PLACEHOLDER_SP_ENTITY" \
      --arg acs "$PLACEHOLDER_SP_ACS" \
      '{
         clientId: $clientId,
         name: "Placeholder AIC SP (replace via register-sp)",
         protocol: "saml",
         enabled: true,
         redirectUris: [$acs, ($clientId + "/*")],
         attributes: {
           "saml.client.signature": "false",
           "saml_name_id_format": "email",
           "saml_force_name_id_format": "true",
           "saml.assertion.signature": "true",
           "saml.server.signature": "true",
           "saml.force.post.binding": "true",
           "saml.authnstatement": "true",
           "saml_assertion_consumer_url_post": $acs
         },
         protocolMappers: [{
           name: "email",
           protocol: "saml",
           protocolMapper: "saml-user-property-mapper",
           consentRequired: false,
           config: {
             "user.attribute": "email",
             "friendly.name": "email",
             "attribute.name": "email",
             "attribute.nameformat": "urn:oasis:names:tc:SAML:2.0:attrname-format:basic"
           }
         }]
       }'
  )"
  kc_expect 201 "create SAML client in ${REALM_IDP}"
  log "created SAML client ${PLACEHOLDER_SP_ENTITY} in ${REALM_IDP}"
}

create_placeholder_idp() {
  if idp_exists "$REALM_SP" "$IDP_ALIAS"; then
    log "identity provider ${IDP_ALIAS} already exists in ${REALM_SP}"
    return 0
  fi
  # entityId is THIS SP's entity id, not the remote IdP's. The remote is
  # idpEntityId. Getting this the wrong way around puts the placeholder IdP
  # URL into the SPSSODescriptor we would hand AIC. Measured.
  kc_req POST "/admin/realms/${REALM_SP}/identity-provider/instances" --data "$(
    jq -n \
      --arg alias "$IDP_ALIAS" \
      --arg spEntity "${PUBLIC_BASE}/realms/${REALM_SP}" \
      --arg idpEntity "$PLACEHOLDER_IDP_ENTITY" \
      --arg sso "$PLACEHOLDER_IDP_SSO" \
      '{
         alias: $alias,
         displayName: "AIC (placeholder — replace via register-idp)",
         providerId: "saml",
         enabled: true,
         trustEmail: true,
         config: {
           entityId: $spEntity,
           idpEntityId: $idpEntity,
           singleSignOnServiceUrl: $sso,
           nameIDPolicyFormat: "urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress",
           principalType: "SUBJECT",
           postBindingResponse: "true",
           postBindingAuthnRequest: "true",
           wantAuthnRequestsSigned: "false",
           wantAssertionsSigned: "true",
           wantAssertionsEncrypted: "false",
           validateSignature: "false"
         }
       }'
  )"
  kc_expect 201 "create identity provider ${IDP_ALIAS}"
  log "created identity provider ${IDP_ALIAS} in ${REALM_SP}"
}

bootstrap() {
  log "bootstrapping realms ${REALM_IDP} and ${REALM_SP}"
  create_realm "$REALM_IDP" "AIC-as-SP peer (Keycloak IdP)"
  create_realm "$REALM_SP" "AIC-as-IdP peer (Keycloak SP via broker)"
  create_user "$REALM_IDP"
  create_user "$REALM_SP"
  create_placeholder_sp_client
  create_placeholder_idp
}

cmd_status() {
  need docker
  need curl
  need jq
  need python3

  if ! container_exists; then
    log "container ${CONTAINER_NAME}: absent"
    log "volume ${VOLUME_NAME}: $(docker volume inspect "$VOLUME_NAME" >/dev/null 2>&1 && echo present || echo absent)"
    return 0
  fi
  if ! container_running; then
    log "container ${CONTAINER_NAME}: present, not running"
    return 0
  fi

  ensure_token
  kc_req GET "/admin/serverinfo"
  kc_expect 200 "serverinfo"
  local version
  version="$(printf '%s' "$KC_BODY" | jq -r '.systemInfo.version')"

  printf 'container:  %s (running)\n' "$CONTAINER_NAME"
  printf 'image:      %s\n' "$(docker inspect -f '{{.Config.Image}}' "$CONTAINER_NAME")"
  printf 'version:    %s\n' "$version"
  printf 'admin UI:   %s/admin\n' "$PUBLIC_BASE"
  printf 'bind:       %s:%s  (admin/admin, local-dev)\n' "$BIND_ADDR" "$HOST_PORT"
  printf '\n'

  local realm
  for realm in "$REALM_IDP" "$REALM_SP"; do
    if ! realm_exists "$realm"; then
      printf 'realm %s: missing (run: %s up)\n' "$realm" "$0"
      continue
    fi
    local uid
    uid="$(user_id "$realm" "$TEST_USER")"
    local kd_total kd_signing kd_enc kd_unspec
    read -r kd_total kd_signing kd_enc kd_unspec <<EOF
$(fetch_descriptor "$realm" | count_key_descriptors)
EOF
    printf 'realm %s\n' "$realm"
    printf '  entityID:     %s/realms/%s\n' "$PUBLIC_BASE" "$realm"
    printf '  descriptor:   %s/realms/%s/protocol/saml/descriptor\n' "$PUBLIC_BASE" "$realm"
    printf '  user:         %s (%s) %s\n' "$TEST_USER" "$TEST_EMAIL" \
      "$([ -n "$uid" ] && echo present || echo MISSING)"
    printf '  KeyDescriptor total=%s signing=%s encryption=%s unspecified=%s\n' \
      "$kd_total" "$kd_signing" "$kd_enc" "$kd_unspec"
    kc_req GET "/admin/realms/${realm}/components?type=org.keycloak.keys.KeyProvider"
    kc_expect 200 "list key providers in ${realm}"
    printf '%s' "$KC_BODY" | jq -r '
      .[] | select(.providerId=="rsa-generated") |
      "  rsa-generated  priority=\(.config.priority[0])  name=\(.name)  id=\(.id)"
    '
    if [ "$realm" = "$REALM_IDP" ]; then
      kc_req GET "/admin/realms/${REALM_IDP}/clients"
      kc_expect 200 "list clients in ${REALM_IDP}"
      printf '%s' "$KC_BODY" | jq -r '
        .[] | select(.protocol=="saml") |
        "  saml client:  \(.clientId)"
      '
    fi
    if [ "$realm" = "$REALM_SP" ]; then
      if idp_exists "$REALM_SP" "$IDP_ALIAS"; then
        printf '  broker alias: %s  (SP descriptor %s/realms/%s/broker/%s/endpoint/descriptor)\n' \
          "$IDP_ALIAS" "$PUBLIC_BASE" "$REALM_SP" "$IDP_ALIAS"
        printf '%s' "$KC_BODY" >/dev/null
        kc_req GET "/admin/realms/${REALM_SP}/identity-provider/instances/${IDP_ALIAS}"
        printf '%s' "$KC_BODY" | jq -r '
          "  idpEntityId:   \(.config.idpEntityId // "unset")",
          "  sp entityId:   \(.config.entityId // "unset")",
          "  sso:           \(.config.singleSignOnServiceUrl // "unset")"
        '
      else
        printf '  broker alias: %s MISSING\n' "$IDP_ALIAS"
      fi
    fi
    printf '\n'
  done
}

cmd_metadata() {
  local realm="${1:-}"
  [ -n "$realm" ] || die "usage: $0 metadata <realm>"
  container_running || die "container not running; try: $0 up"
  local tmp code
  tmp="$(mktemp)"
  code="$(curl -sS -o "$tmp" -w '%{http_code}' \
    "${CURL_BASE}/realms/${realm}/protocol/saml/descriptor")"
  if [ "$code" != "200" ]; then
    rm -f "$tmp"
    die "GET /realms/${realm}/protocol/saml/descriptor -> HTTP ${code}"
  fi
  pretty_xml <"$tmp"
  rm -f "$tmp"
}

cmd_sp_metadata() {
  container_running || die "container not running; try: $0 up"
  local tmp code
  tmp="$(mktemp)"
  code="$(curl -sS -o "$tmp" -w '%{http_code}' \
    "${CURL_BASE}/realms/${REALM_SP}/broker/${IDP_ALIAS}/endpoint/descriptor")"
  if [ "$code" != "200" ]; then
    rm -f "$tmp"
    die "GET broker descriptor -> HTTP ${code} (is identity provider ${IDP_ALIAS} present?)"
  fi
  pretty_xml <"$tmp"
  rm -f "$tmp"
}

rsa_generated_json() {
  local realm="$1"
  kc_req GET "/admin/realms/${realm}/components?type=org.keycloak.keys.KeyProvider"
  kc_expect 200 "list key providers in ${realm}"
  printf '%s' "$KC_BODY" | jq '[.[] | select(.providerId=="rsa-generated")]'
}

cmd_rotate_add() {
  local realm="${1:-$REALM_IDP}"
  container_running || die "container not running; try: $0 up"
  local parent max_pri new_pri before after
  parent="$(realm_uuid "$realm")"
  before="$(signing_count "$realm")"
  max_pri="$(
    rsa_generated_json "$realm" | jq -r '
      if length == 0 then 0
      else [.[] | .config.priority[0] | tonumber] | max
      end
    '
  )"
  new_pri=$((max_pri + 100))
  kc_req POST "/admin/realms/${realm}/components" --data "$(
    jq -n --arg parent "$parent" --arg pri "$new_pri" --arg name "rsa-generated-rotation-${new_pri}" \
      '{
         name: $name,
         providerId: "rsa-generated",
         providerType: "org.keycloak.keys.KeyProvider",
         parentId: $parent,
         config: {
           priority: [$pri],
           enabled: ["true"],
           active: ["true"],
           algorithm: ["RS256"]
         }
       }'
  )"
  kc_expect 201 "add rsa-generated at priority ${new_pri}"
  after="$(signing_count "$realm")"
  log "added rsa-generated-rotation-${new_pri} in ${realm}"
  log "signing KeyDescriptors: ${before} -> ${after}"
  printf '%s\n' "$after"
}

cmd_rotate_rm() {
  local realm="${1:-$REALM_IDP}"
  container_running || die "container not running; try: $0 up"
  local before after old_id old_pri count
  count="$(rsa_generated_json "$realm" | jq 'length')"
  [ "$count" -ge 2 ] || die "only ${count} rsa-generated provider(s) in ${realm}; refuse to remove the last signing key"
  old_id="$(
    rsa_generated_json "$realm" | jq -r '
      min_by(.config.priority[0] | tonumber) | .id
    '
  )"
  old_pri="$(
    rsa_generated_json "$realm" | jq -r '
      min_by(.config.priority[0] | tonumber) | .config.priority[0]
    '
  )"
  before="$(signing_count "$realm")"
  kc_req DELETE "/admin/realms/${realm}/components/${old_id}"
  kc_expect 204 "delete rsa-generated ${old_id}"
  after="$(signing_count "$realm")"
  log "removed rsa-generated id=${old_id} priority=${old_pri} in ${realm}"
  log "signing KeyDescriptors: ${before} -> ${after}"
  printf '%s\n' "$after"
}

cmd_verify_rotate() {
  local realm="${1:-$REALM_IDP}"
  container_running || die "container not running; try: $0 up"
  local c0 c1 c2
  c0="$(signing_count "$realm")"
  log "verify-rotate ${realm}: before=${c0}"
  cmd_rotate_add "$realm" >/dev/null
  c1="$(signing_count "$realm")"
  log "verify-rotate ${realm}: after-add=${c1}"
  cmd_rotate_rm "$realm" >/dev/null
  c2="$(signing_count "$realm")"
  log "verify-rotate ${realm}: after-rm=${c2}"
  printf 'signing KeyDescriptors: %s -> %s -> %s\n' "$c0" "$c1" "$c2"
  if [ "$c1" -ne $((c0 + 1)) ] || [ "$c2" -ne "$c0" ]; then
    die "rotation did not move the descriptor as expected"
  fi
}

cmd_register_sp() {
  local file="${1:-}"
  [ -n "$file" ] && [ -f "$file" ] || die "usage: $0 register-sp <spssodescriptor.xml>"
  container_running || die "container not running; try: $0 up"
  ensure_token
  local tmp conv client_id existing
  tmp="$(mktemp)"
  conv="$(mktemp)"
  # Converter consumes a JSON string that is the XML (Content-Type application/json,
  # body = the descriptor). Measured 200 for SPSSODescriptor, 500 for IDPSSODescriptor.
  curl -sS -o "$tmp" -w '%{http_code}' -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "Content-Type: application/json" \
    --data-binary @"$file" \
    "${CURL_BASE}/admin/realms/${REALM_IDP}/client-description-converter" >"${tmp}.code"
  KC_CODE="$(cat "${tmp}.code")"
  if [ "$KC_CODE" != "200" ]; then
    die "client-description-converter HTTP ${KC_CODE} (need an SPSSODescriptor, not an IdP descriptor): $(head -c 400 "$tmp")"
  fi
  # Force email NameID so the harness NameID is predictable even if the
  # imported metadata listed persistent first.
  jq '.attributes["saml_name_id_format"] = (.attributes["saml_name_id_format"] // "email")
      | .attributes["saml_force_name_id_format"] = "true"
      | .name = (.name // "Imported AIC SP")' "$tmp" >"$conv"
  client_id="$(jq -r '.clientId' "$conv")"
  existing="$(client_uuid "$REALM_IDP" "$client_id")"
  if [ -n "$existing" ]; then
    kc_req PUT "/admin/realms/${REALM_IDP}/clients/${existing}" --data-binary @"$conv"
    kc_expect 204 "update SAML client ${client_id}"
    log "updated SAML client ${client_id} in ${REALM_IDP}"
  else
    kc_req POST "/admin/realms/${REALM_IDP}/clients" --data-binary @"$conv"
    kc_expect 201 "create SAML client ${client_id}"
    log "created SAML client ${client_id} in ${REALM_IDP}"
  fi
  rm -f "$tmp" "${tmp}.code" "$conv"
}

cmd_register_idp() {
  local file="${1:-}"
  [ -n "$file" ] && [ -f "$file" ] || die "usage: $0 register-idp <idpssodescriptor.xml>"
  container_running || die "container not running; try: $0 up"
  ensure_token
  local imported merged
  imported="$(mktemp)"
  merged="$(mktemp)"
  # Multipart file upload. JSON fromUrl 500s from this container (the server
  # cannot fetch the host's published port); AIC cannot fetch localhost either,
  # so file/paste is the only path that matters.
  curl -sS -o "$imported" -w '%{http_code}' -X POST \
    -H "Authorization: Bearer ${TOKEN}" \
    -F "providerId=saml" \
    -F "file=@${file};type=application/xml" \
    "${CURL_BASE}/admin/realms/${REALM_SP}/identity-provider/import-config" >"${imported}.code"
  KC_CODE="$(cat "${imported}.code")"
  [ "$KC_CODE" = "200" ] || die "import-config HTTP ${KC_CODE}: $(cat "$imported")"

  local sp_entity existing_cfg
  sp_entity="${PUBLIC_BASE}/realms/${REALM_SP}"
  existing_cfg='{}'
  if idp_exists "$REALM_SP" "$IDP_ALIAS"; then
    sp_entity="$(printf '%s' "$KC_BODY" | jq -r '.config.entityId // empty')"
    [ -n "$sp_entity" ] || sp_entity="${PUBLIC_BASE}/realms/${REALM_SP}"
    existing_cfg="$(printf '%s' "$KC_BODY" | jq -c '.config')"
  fi

  # import-config returns idpEntityId (remote) and does NOT return entityId
  # (this SP). It also picks the FIRST NameIDFormat in the descriptor, which
  # for Keycloak is persistent — override to email so NameID stays predictable.
  # Overlay onto any existing config so keys import-config omits
  # (wantAssertionsSigned, principalType, …) are not wiped.
  jq -n \
    --arg alias "$IDP_ALIAS" \
    --arg spEntity "$sp_entity" \
    --argjson existing "$existing_cfg" \
    --slurpfile imported "$imported" \
    '{
       alias: $alias,
       displayName: "AIC",
       providerId: "saml",
       enabled: true,
       trustEmail: true,
       config: ($existing + $imported[0] + {
         entityId: $spEntity,
         nameIDPolicyFormat: "urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress",
         wantAssertionsSigned: "true"
       })
     }' >"$merged"

  if idp_exists "$REALM_SP" "$IDP_ALIAS"; then
    kc_req PUT "/admin/realms/${REALM_SP}/identity-provider/instances/${IDP_ALIAS}" \
      --data-binary @"$merged"
    kc_expect 204 "update identity provider ${IDP_ALIAS}"
    log "updated identity provider ${IDP_ALIAS} in ${REALM_SP} from ${file}"
  else
    kc_req POST "/admin/realms/${REALM_SP}/identity-provider/instances" \
      --data-binary @"$merged"
    kc_expect 201 "create identity provider ${IDP_ALIAS}"
    log "created identity provider ${IDP_ALIAS} in ${REALM_SP} from ${file}"
  fi
  rm -f "$imported" "${imported}.code" "$merged"
}

cmd_wire_loopback() {
  container_running || die "container not running; try: $0 up"
  local idp_xml sp_xml
  idp_xml="$(mktemp)"
  sp_xml="$(mktemp)"
  curl -fsS "${CURL_BASE}/realms/${REALM_IDP}/protocol/saml/descriptor" >"$idp_xml"
  cmd_register_idp "$idp_xml"
  # After import, wantAuthnRequestsSigned becomes true (copied from the IdP
  # descriptor) and the SP metadata gains a signing KeyDescriptor. Re-export
  # and register that as the SAML client on the IdP side.
  curl -fsS "${CURL_BASE}/realms/${REALM_SP}/broker/${IDP_ALIAS}/endpoint/descriptor" >"$sp_xml"
  cmd_register_sp "$sp_xml"
  rm -f "$idp_xml" "$sp_xml"
  log "loopback wired: ${REALM_SP} brokers from ${REALM_IDP}"
  log "initiate at ${PUBLIC_BASE}/realms/${REALM_SP}/account/"
}

main() {
  local cmd="${1:-help}"
  if [ $# -gt 0 ]; then
    shift
  fi
  case "$cmd" in
    up) up_cmd_check; cmd_up "$@" ;;
    down) cmd_down "$@" ;;
    reset) up_cmd_check; cmd_reset "$@" ;;
    status) cmd_status "$@" ;;
    metadata) cmd_metadata "$@" ;;
    sp-metadata) cmd_sp_metadata "$@" ;;
    rotate-add) cmd_rotate_add "$@" ;;
    rotate-rm) cmd_rotate_rm "$@" ;;
    verify-rotate) cmd_verify_rotate "$@" ;;
    register-sp) cmd_register_sp "$@" ;;
    register-idp) cmd_register_idp "$@" ;;
    wire-loopback) cmd_wire_loopback "$@" ;;
    help|-h|--help) usage ;;
    *)
      usage >&2
      die "unknown command: ${cmd}"
      ;;
  esac
}

up_cmd_check() {
  need docker
  need curl
  need jq
  need python3
}

main "$@"
