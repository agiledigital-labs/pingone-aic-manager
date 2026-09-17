# SAML user test plan

A walk through everything `aic saml` does, in the order that builds confidence:
offline first, then read-only against the tenant, then writes in a throwaway
realm, then a certificate rotation end to end, then real SAML authentication in
both directions against a local Keycloak.

Every command here was run against a live sandbox tenant on 2026-09-18 unless
the step says otherwise. Where a step has a **watch for** note, that is the
thing worth looking at rather than the exit code.

## Before you start

```sh
aic login                     # the agent must be unlocked
aic status                    # expect "unlocked: true" and your tenant
cargo build                   # PATH prefers target/debug/aic in this repo
```

**Use `alpha` as the throwaway realm.** It had no SAML entity providers at all,
so anything you find in it afterwards is yours. `bravo` holds real client
federations — read from it, never write to it.

Every write step below has a matching cleanup step in
[Cleaning up](#cleaning-up). Do those even if a step fails part way.

---

## Part A — offline, no tenant involved

Nothing here leaves your disk. Safe to run anywhere, locked agent or not.

### A1 · Read a real Entra metadata document

```sh
aic saml metadata inspect src/saml/fixtures/entra-federationmetadata.xml
```

**Watch for:** the two `RoleDescriptor` blocks reported as WS-Fed rather than
SAML 2.0, and the enveloped `<ds:Signature>`. Those three sections are what AIC's
importer rejects, and this is the pain point the next step exists for.

### A2 · Strip it down to what AIC will accept

```sh
aic saml metadata sanitise src/saml/fixtures/entra-federationmetadata.xml \
  --out /tmp/entra-clean.xml
```

**Watch for:** each removal named with its line number and its reason. Then
confirm the tool spliced rather than rewrote:

```sh
diff <(aic saml metadata inspect /tmp/entra-clean.xml) \
     <(aic saml metadata inspect src/saml/fixtures/entra-federationmetadata.xml)
```

The surviving bytes should be byte-identical to the original's, not
reserialised — that is what keeps a signature you chose to keep verifiable.

### A3 · Prove it fails closed

Truncate the document mid-element and confirm it is refused rather than
half-processed:

```sh
head -c 400 src/saml/fixtures/entra-federationmetadata.xml > /tmp/truncated.xml
aic saml metadata inspect /tmp/truncated.xml; echo "exit $?"
```

**Watch for:** a non-zero exit and a refusal that names the problem. A tool that
reported something confident about a truncated document is the failure mode here.

---

## Part B — read-only against the tenant

Nothing in Part B writes. Run it against `bravo` to see real federations.

### B1 · Inventory

```sh
aic saml list --realm bravo
aic saml show <ENTITY-ID-FROM-THE-LIST> --realm bravo
```

### B2 · Export metadata — the thing the console will not do

```sh
aic saml metadata export <ENTITY-ID> --realm bravo --out /tmp/exported.xml
aic saml metadata inspect /tmp/exported.xml
```

**Watch for:** this works with the agent **locked** as well as unlocked — the
export endpoint takes no bearer. Try it after `aic session logout` if you want
to confirm.

The interesting failure is invisible without the tool: an export failure comes
back as **HTTP 200** with an `ERROR :` string in the body. Ask for an entity
that does not exist and check you get a refusal rather than a file:

```sh
aic saml metadata export https://nope.example.com --realm bravo --out /tmp/nope.xml
echo "exit $?"; ls /tmp/nope.xml
```

**Watch for:** non-zero exit, and **no file created**.

### B3 · Circles of trust, and the caveat that matters most

```sh
aic saml cot list --realm bravo
```

**Watch for:** circles showing **0 providers** while being live, working
federations. That is not a bug in the listing — AM stores membership twice, in
the CoT document's `trustedProviders` (which REST returns) and in each entity's
`cotlist` in extended metadata (which REST never exposes). The runtime trust
check reads the half you cannot see. The caveat printed under the table says so
every time, on purpose.

---

## Part C — writes, in the throwaway realm

### C1 · Create a hosted SP

```sh
aic saml create-hosted 'https://sp-a.example.com' \
  --realm alpha --role sp --meta-alias /alpha/rotate-test
aic saml show 'https://sp-a.example.com' --realm alpha
```

**Watch for:** the output telling you the 201 body is a stub and nothing was
read back. Both the entity ID and the metaAlias are imposed by `aic`; AM
requires neither, and without the second it fails with a 500 naming no field.

### C2 · Preview an import, then do it

```sh
aic saml import /tmp/entra-clean.xml --realm alpha --dry-run
```

**Watch for:** every entity id listed exactly, the preflight against **both**
collections, and `dry run: nothing was sent`. The preview cannot write — it
holds no permit, so the write path will not compile for it, rather than being
branched around.

```sh
aic saml import /tmp/entra-clean.xml --realm alpha
```

**Watch for:** the entity id echoed back byte-identically, trailing `/` and all
— Entra ids end in one, and this was the open question until it was measured on
2026-09-18. Then the caveat that no post-import `cotlist` verification is
possible, because `importEntity` rewrites extended metadata.

### C3 · Confirm the import collided safely

Run the same import again:

```sh
aic saml import /tmp/entra-clean.xml --realm alpha; echo "exit $?"
```

**Watch for:** a refusal naming the existing entity, and **no** `--force` that
offers to delete and re-import. There is deliberately no such flag: the
`cotlist` cannot be read back or restored, so a delete-and-recreate would
silently drop a federation's trust and nothing could detect it.

---

## Part D — rotate a signing certificate

This is the part that answers "does an upload replace the certificate or add a
second one?" The answer is neither, because **the certificate is not in the
entity** — the entity holds a `secretIdIdentifier`, a label into AM's secret
store, backed on AIC by an ESV secret. A rollover is an ESV secret *version*
operation. There is nothing to upload and no metadata to push.

### D0 · Make two throwaway key pairs

```sh
cd /tmp
for n in 1 2; do
  openssl req -x509 -newkey rsa:2048 -keyout k$n.pem -out c$n.pem -days 30 \
    -nodes -subj "/CN=aic-rotate-test-$n"
  cat k$n.pem c$n.pem > pair$n.pem
  printf 'cert%s ' $n; openssl x509 -in c$n.pem -outform DER | sha256sum
done
```

Keep those two fingerprints on screen. Every step below is checked against them.

### D1 · Where it stands now

```sh
aic saml rotate status 'https://sp-a.example.com' --realm alpha
```

**Watch for:** `phase unconfigured`, and one published certificate that is the
**realm-wide default** — shared with every other entity in the realm. Note its
fingerprint; it is your baseline, and the fact that it is a known, named
certificate is what makes the next step's change observable rather than assumed.

### D2 · Preview, then perform, the one-time setup

```sh
aic saml rotate init 'https://sp-a.example.com' --realm alpha \
  --identifier sprotatetest --secret-id esv-saml-sprotatetest-signing \
  --key-file /tmp/pair1.pem --dry-run
```

**Watch for:** three steps planned — the entity `PUT`, the ESV secret, the label
mapping — and the certificate fingerprint matching your cert1.

> `--identifier` takes **letters and digits only**. AM rejects `-` and `_` with
> `400 Invalid character present in Secret ID Identifier` and names no flag,
> which is why `aic` refuses them first.

```sh
aic saml rotate init 'https://sp-a.example.com' --realm alpha \
  --identifier sprotatetest --secret-id esv-saml-sprotatetest-signing \
  --description 'throwaway rotate test, delete me' --key-file /tmp/pair1.pem
```

**Watch for:** `(read back and compared whole)` on the entity line. The entity
`PUT` is a full replace with no `If-Match` — sending `{"entityId": "<same>"}`
is a 200 that deletes the role block — so it re-reads and compares the whole
document rather than trusting the status code.

Then verify from the tenant's own metadata rather than from what `aic` claimed:

```sh
aic saml metadata export 'https://sp-a.example.com' --realm alpha --out /tmp/d2.xml
aic saml metadata inspect /tmp/d2.xml
```

**Watch for:** the signing certificate is now **cert1**, the encryption
certificate is untouched, and **no restart was needed** — that is the
`--no-placeholders` ESV secret doing its job.

### D3 · Stage the second certificate

```sh
aic saml rotate stage 'https://sp-a.example.com' --realm alpha \
  --key-file /tmp/pair2.pem
```

**Watch for:** `version 2 added`, then **both** fingerprints published. This is
the window in which a peer can load the new certificate while the old one still
works. Confirm independently:

```sh
aic saml metadata export 'https://sp-a.example.com' --realm alpha --out /tmp/d3.xml
aic saml metadata inspect /tmp/d3.xml
```

**Watch for:** two `use="signing"` KeyDescriptors, cert2 and cert1.

### D4 · Read what the tool admits it cannot know

```sh
aic saml rotate status 'https://sp-a.example.com' --realm alpha
```

**Watch for:** cert2 attributed to "ESV secret version 2, staged here <time>",
and cert1 marked "(no local record of which version holds it)". That asymmetry
is the honest answer: secret values are write-only and AM publishes no
`<ds:KeyName>`, so nothing on the tenant ties a certificate to a version.
`aic` knows about cert2 only because **this install staged it** and confirmed it
from the tenant's export afterwards. It never guesses the other by elimination.

This is the point at which you would hand the peer the two-certificate metadata
and wait for them to load it. In production that wait is the whole reason
`stage` and `complete` are separate verbs.

### D5 · Close the window

```sh
aic saml rotate complete 'https://sp-a.example.com' --realm alpha
```

**Watch for:** a **refusal**. Retiring a published certificate needs confirmation
at a terminal, or `--force`. Then:

```sh
aic saml rotate complete 'https://sp-a.example.com' --realm alpha --force
aic saml rotate status 'https://sp-a.example.com' --realm alpha
```

**Watch for:** `phase settled`, one published certificate (cert2), and
version 1 `DISABLED` — **not destroyed**. The output tells you how to put it
back. Nothing in this whole verb destroys anything.

Confirm from the tenant one more time:

```sh
aic saml metadata export 'https://sp-a.example.com' --realm alpha --out /tmp/d5.xml
aic saml metadata inspect /tmp/d5.xml
```

**Watch for:** cert1 gone, cert2 alone. Checking the *count* would not have been
enough — a `complete` that disabled the wrong version also leaves exactly one
certificate published. It is the identity that matters.

### D6 · Undo it, to prove nothing was lost

```sh
aic esv secret enable esv-saml-sprotatetest-signing 1
aic saml rotate status 'https://sp-a.example.com' --realm alpha
```

**Watch for:** both certificates published again. Re-disable with
`aic esv secret disable esv-saml-sprotatetest-signing 1` when you are done
looking.

---

## Part E — the three traps, seen deliberately

These are AM behaviours, not `aic` behaviours, and each one silently strands
something. Worth seeing once so you recognise them on a client tenant.

### E1 · Deleting the entity does not remove its secret or its mapping

```sh
aic saml delete 'https://sp-a.example.com' --realm alpha --force
aic secretmap list | grep sprotatetest
aic esv secret list | grep sprotatetest
```

**Watch for:** the mapping and the ESV secret both still there, now naming an
entity that does not exist. Nothing in AM cleans these up.

### E2 · Repointing an entity's identifier orphans the old mapping

`aic saml rotate init` refuses to repoint an entity that already has an
identifier, for exactly this reason. Try it on a fresh entity to read the
refusal — it names `secretmap remove` as the deliberate way through.

### E3 · The orphan used to be undeletable

```sh
aic secretmap remove \
  am.applications.federation.entity.providers.saml2.sprotatetest.signing --force
```

**Watch for:** it works. It did not before this sprint: `remove` consulted the
schema enum to decide whether a label existed, and a label vanishes from that
enum the moment the entity stops naming it — so the one mapping you most needed
to delete was the one the tool said was not there.

---

## Part F — real SAML authentication, both directions

Parts A–E exercise configuration. Part F exercises whether a browser actually
authenticates. It needs Docker and a few minutes.

```sh
scripts/saml-harness/harness.sh up
scripts/saml-harness/harness.sh status
```

### F1 · AIC as SP, Keycloak as IdP

```sh
scripts/saml-harness/harness.sh metadata aic-idp > /tmp/kc-idp.xml
aic saml import /tmp/kc-idp.xml --realm alpha
aic saml metadata export '<your-aic-sp-entity-id>' --realm alpha --out /tmp/aic-sp.xml
scripts/saml-harness/harness.sh register-sp /tmp/aic-sp.xml
```

### F2 · AIC as IdP, Keycloak as SP

```sh
scripts/saml-harness/harness.sh sp-metadata > /tmp/kc-sp.xml
aic saml import /tmp/kc-sp.xml --realm alpha
aic saml metadata export '<your-aic-idp-entity-id>' --realm alpha --out /tmp/aic-idp.xml
scripts/saml-harness/harness.sh register-idp /tmp/aic-idp.xml
```

Both directions need the circle-of-trust step done by hand — there is no CoT
write verb, and `docs/api/06-saml.md` gives the REST call. See
`docs/saml-test-harness.md` for the full walkthrough and the browser steps.

### F3 · Rotation, measured rather than assumed

```sh
scripts/saml-harness/harness.sh verify-rotate aic-idp
```

**Watch for:** this adds a key, removes one, and asserts the **surviving signing
key is the added one and is not a pre-rotation one**, by certificate
fingerprint. A count of `KeyDescriptor`s would pass whether or not the rotation
worked, which is why it does not use one.

---

## Cleaning up

Run this whether or not you got to the end. It should leave `alpha` exactly as
you found it — empty.

```sh
aic saml delete 'https://sp-a.example.com' --realm alpha --force
aic saml delete 'https://sts.windows.net/<tenant-guid>/' --realm alpha --force
aic secretmap remove \
  am.applications.federation.entity.providers.saml2.sprotatetest.signing --force
aic esv secret delete esv-saml-sprotatetest-signing --force

# confirm
aic saml list --realm alpha                     # expect: none
aic secretmap list | grep -c sprotatetest       # expect: 0
aic esv secret list  | grep -c sprotatetest     # expect: 0

scripts/saml-harness/harness.sh down
```

The rotation journal at `.aic/saml-rotations.json` is local, holds no key
material, and is gitignored. It should be `[]` when nothing is mid-rollover.

## What to report back

Anything where the **watch for** note did not match what you saw. In particular:
a command that exited zero while the thing it described did not happen, a
refusal you think was wrong, or a message that told you what happened without
telling you what to do next.
