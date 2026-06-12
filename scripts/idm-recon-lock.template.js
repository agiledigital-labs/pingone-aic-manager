/**
 * IDM advisory-lock template (for guarding reconById against concurrent runs).
 *
 * Strategy: acquire a lock by CREATING a managed "lock" object whose _id is the
 * lock key. openidm.create() is a CREST CreateRequest with NO upsert fallback —
 * a duplicate _id throws PreconditionFailedException ("Entry Already Exists"),
 * verified 2026-06-10 (see docs/api/10-managed-objects.md). That failure is what
 * tells us "someone else holds the lock". The stored timestamp gives us an
 * auto-expiry so a lock orphaned by catastrophic failure (where `finally` never
 * ran) does not wedge everything forever — a later caller reclaims it.
 *
 * NOTE: whether openidm.create *reliably* fails on a duplicate in clustered prod
 * is the subject of an open Ping query. The retry + expiry logic here is the
 * belt-and-suspenders that makes this usable either way; it does not by itself
 * make the lock airtight against a multi-master replication add-add race.
 *
 * ---------------------------------------------------------------------------
 * VERIFIED on AIC IDM (sandbox, 2026-06-10):
 *   - java.lang.Thread.sleep(ms) works (Java access permitted; measured ~263ms
 *     for a 250ms request).
 *   - The managed/alpha_lock type exists with fields lockKey/owner/acquiredAt/
 *     expiresAt (no hooks, no sync mapping → creating a lock has no side effects).
 *   - All paths below were exercised in-engine via a scripted-endpoint harness:
 *     acquire/release, finally-release-on-throw, contention timeout (503),
 *     stale-lock reclaim, owner-fenced release, lease renewal + its fence, and
 *     12-way parallel serialization (all serialized through one lock).
 *
 * IDM engine syntax: let/const/arrow/template OK. Do NOT use default params or
 * `const` in a for-initializer (both compile-fail on IDM) — there are no for
 * loops here, so `const` is used throughout except for reassigned bindings.
 * ---------------------------------------------------------------------------
 *
 * alpha_lock object shape — one instance == one held lock:
 *   {
 *     "_id":        "<lockKey>",   // e.g. "<mapping>-<objectId>"; maps to the DS uid RDN
 *     "lockKey":    "<lockKey>",   // string, mirrors _id
 *     "owner":      "<token>",     // string fencing token (see newOwnerToken)
 *     "acquiredAt": 1781066042733, // number, epoch ms
 *     "expiresAt":  1781066072733, // number, epoch ms (acquiredAt + ttlMs)
 *     "_rev":       "…"            // server-assigned; used for fenced delete/update
 *   }
 * Managed type: no onCreate/onUpdate/onDelete hooks, no sync mapping (so creating
 * a lock has no side effects); properties are not `searchable` because every
 * acquire/release/reclaim reads by _id. A cleanup sweep can list with
 * _queryFilter=true and drop instances whose expiresAt < now client-side.
 * ---------------------------------------------------------------------------
 */

/**
 * Run `work` while holding an advisory lock. Releases in `finally`, so only a
 * catastrophic failure (JVM death, etc.) can leave a lock undeleted — and even
 * that self-heals once `ttlMs` elapses and the next caller reclaims it.
 *
 * `work` is called as work(renew): call renew() at checkpoints in a long
 * operation to extend the lease (see "TTL vs work duration" below).
 *
 * @param {string}   lockKey  e.g. objectName + "-" + objectId. Becomes the _id,
 *                            so keep it to a DS-safe naming value (avoid commas,
 *                            '+', leading/trailing spaces, control chars).
 * @param {object}   options
 *        options.container    managed lock container (default "managed/alpha_lock")
 *        options.maxRetries   acquisition attempts after the first (default 5)
 *        options.retryPauseMs pause between attempts, ms (default 1000)
 *        options.ttlMs        lock auto-expiry, ms (default 30000). MUST exceed
 *                             the worst-case duration of `work` — see caveats.
 *        options.owner        fencing token; defaults to a per-call unique id
 *        options.confirmOwnership  read the lock back after create and require
 *                             owner === ours before proceeding (default true).
 *                             Defence-in-depth against a multi-master "both
 *                             creates succeeded" race — see caveat below.
 * @param {function} work     the protected operation, called as work(renew);
 *                            its return value is returned
 * @returns whatever `work` returns
 * @throws  a CREST-style error object if the lock cannot be acquired in time,
 *          or whatever `work` throws (lock is still released first)
 *
 * TTL vs work duration: if `work` runs longer than ttlMs, another caller will
 * treat the lock as stale and steal it → two operations run concurrently. Either
 * set ttlMs comfortably above the worst case, or call renew() from inside `work`
 * before the lease is half gone. The owner fence stops the slow holder from
 * deleting the new holder's lock on the way out, but it cannot prevent the
 * overlap itself. A single openidm.action('recon','reconById',…) call cannot be
 * checkpointed mid-call, so for that case prefer a generous ttlMs.
 *
 * Why no automatic background heartbeat: an IDM script invocation is single-
 * threaded; spawning a renewal thread is unsafe/unsupported here. Renewal is
 * therefore cooperative (work calls renew()).
 *
 * confirmOwnership caveat: the read-back compares `owner`, not mere existence
 * (a creator always finds its own _id). It catches the case where a concurrent
 * create won the canonical record — but only once replication has converged;
 * an immediate read can still return our own write from our own replica, so it
 * NARROWS the multi-master window rather than closing it. A failed confirmation
 * makes us back off instead of proceeding, but in a lagging-replica read it may
 * make us back off a lock we actually hold until it expires. It complements,
 * not replaces, confirmation that openidm.create is exclusive.
 */
function withLock(lockKey, options, work) {
  const opts = options || {};
  const container =
    opts.container != null ? opts.container : "managed/alpha_lock";
  const maxRetries = opts.maxRetries != null ? opts.maxRetries : 5;
  const retryPauseMs = opts.retryPauseMs != null ? opts.retryPauseMs : 1000;
  const ttlMs = opts.ttlMs != null ? opts.ttlMs : 30000;
  const owner = opts.owner != null ? opts.owner : newOwnerToken();

  if (!lockKey) {
    throw { code: 400, message: "withLock: lockKey is required" };
  }

  const confirmOwnership =
    opts.confirmOwnership != null ? opts.confirmOwnership : true;

  const lockPath = container + "/" + lockKey;
  acquireLock(
    container,
    lockKey,
    lockPath,
    owner,
    ttlMs,
    maxRetries,
    retryPauseMs,
    confirmOwnership
  );

  function renew() {
    return renewLease(container, lockKey, owner, ttlMs);
  }

  try {
    return work(renew);
  } finally {
    releaseLock(lockPath, owner);
  }
}

/**
 * Try to create the lock object, retrying (and reclaiming stale locks) until it
 * succeeds or the retry budget is exhausted. Creation is the acquisition: a
 * duplicate _id means the lock is held (see {@link isAlreadyExists}).
 *
 * @param {string} container    managed lock container, e.g. "managed/alpha_lock"
 * @param {string} lockKey      the lock _id to create
 * @param {string} lockPath     container + "/" + lockKey (passed through to reclaim)
 * @param {string} owner        fencing token stored on the lock
 * @param {number} ttlMs        lease length written into expiresAt
 * @param {number} maxRetries   retry attempts after the first
 * @param {number} retryPauseMs pause between attempts, ms
 * @param {boolean} confirmOwnership read the lock back and require owner === ours
 *          before treating the create as a successful acquisition
 * @returns the created lock resource (includes _id and _rev)
 * @throws  {object} a CREST-style 503 error when the budget is exhausted, or the
 *          underlying error if create fails for any reason other than "exists"
 */
function acquireLock(
  container,
  lockKey,
  lockPath,
  owner,
  ttlMs,
  maxRetries,
  retryPauseMs,
  confirmOwnership
) {
  let attempt = 0;
  while (true) {
    const nowMs = now();
    let created = null;
    try {
      created = openidm.create(container, lockKey, {
        lockKey: lockKey,
        owner: owner,
        acquiredAt: nowMs,
        expiresAt: nowMs + ttlMs,
      });
    } catch (e) {
      // Log the error so we can refine the isAlreadyExists() classifier if we
      // see something unexpected in the logs.
      logger.info(
        "withLock: failed to get lock {} (attempt {}): {}",
        lockPath,
        attempt,
        e
      );
      if (!isAlreadyExists(e)) {
        // Not a "lock held" signal — a real error (bad container, policy, perms).
        throw e;
      }
      // Lock is held. If it is past its expiry, reclaim it so a crashed holder
      // can't wedge us forever, then fall through to back off and retry.
      reclaimIfExpired(lockPath);
    }

    if (created) {
      // Create reported success. Optionally confirm we truly own the canonical
      // record — guards against a multi-master race where another writer's
      // create also "succeeded" and won the _id (see withLock JSDoc caveat).
      if (!confirmOwnership || ownsCanonicalLock(lockPath, owner)) {
        return created;
      }
      // We created but do not own the canonical record: we lost the race. Do NOT
      // delete it (the owner fence belongs to the winner); back off and contend.
      logger.warn(
        "withLock: created {} but canonical owner differs — backing off (possible multi-master race)",
        lockPath
      );
    }

    if (attempt >= maxRetries) {
      throw {
        code: 503,
        message:
          'Could not acquire lock "' +
          lockKey +
          '" after ' +
          (maxRetries + 1) +
          " attempts",
        detail: { lockKey: lockKey, container: container },
      };
    }
    attempt++;
    sleep(retryPauseMs);
  }
}

/**
 * Read the lock back by _id and report whether WE own it. Used to confirm an
 * acquisition: compares `owner` (not mere existence — the creator always finds
 * its own _id). Fail-closed: any read error returns false, so we treat an
 * unconfirmable lock as not-ours and decline rather than risk a double-acquire.
 *
 * @param {string} lockPath container + "/" + lockKey
 * @param {string} owner    our fencing token
 * @returns {boolean} true iff the current record exists and its owner is ours
 */
function ownsCanonicalLock(lockPath, owner) {
  try {
    const current = openidm.read(lockPath);
    return !!current && current.owner === owner;
  } catch (e) {
    return false;
  }
}

/**
 * Release the lock, but only if we still own it — our lease may have expired and
 * been reclaimed by another caller, whose lock we must not delete. Deletes by the
 * current _rev so we never clobber a concurrently-refreshed lock. Never throws:
 * a release failure is logged, not propagated, so it can't mask the `work` result.
 *
 * @param {string} lockPath container + "/" + lockKey
 * @param {string} owner    our fencing token; deletion happens only on a match
 * @returns {void}
 */
function releaseLock(lockPath, owner) {
  try {
    const current = openidm.read(lockPath);
    if (current && current.owner === owner) {
      // delete by the current _rev so we never clobber a lock someone else now holds
      openidm.delete(lockPath, current._rev);
    } else if (current) {
      logger.warn(
        "withLock: not releasing {} — owned by {} not {} (lease likely expired)",
        lockPath,
        current.owner,
        owner
      );
    }
  } catch (e) {
    // Never let a release failure mask the outcome of `work`; just log it.
    logger.warn("withLock: failed to release {}: {}", lockPath, String(e));
  }
}

/**
 * If a lock exists and is past its expiresAt, delete it so a crashed holder can't
 * wedge callers forever. Deletes by the read _rev, so when two callers both judge
 * a lock stale only one delete succeeds and the acquire loop still converges to a
 * single winner. Never throws — a missing/already-reclaimed lock is a no-op.
 *
 * @param {string} lockPath container + "/" + lockKey
 * @returns {void}
 */
function reclaimIfExpired(lockPath) {
  let existing;
  try {
    existing = openidm.read(lockPath);
  } catch (e) {
    return; // already gone — next create attempt may win
  }
  if (!existing) {
    return;
  }
  if (typeof existing.expiresAt === "number" && now() >= existing.expiresAt) {
    try {
      openidm.delete(lockPath, existing._rev);
      logger.warn(
        "withLock: reclaimed expired lock {} (owner {}, expired {})",
        lockPath,
        existing.owner,
        existing.expiresAt
      );
    } catch (e) {
      // Someone else reclaimed/refreshed it first — fine, just retry the loop.
    }
  }
}

/**
 * Extend our lease by ttlMs. Owner-fenced: refuses if we no longer hold the lock.
 * Call from inside `work` (via the renew() passed to it) at checkpoints.
 */
function renewLease(container, lockKey, owner, ttlMs) {
  const lockPath = container + "/" + lockKey;
  const current = openidm.read(lockPath);
  if (!current) {
    throw {
      code: 410,
      message: "cannot renew: lock gone",
      detail: { lockKey: lockKey },
    };
  }
  if (current.owner !== owner) {
    throw {
      code: 409,
      message: "cannot renew: not the owner",
      detail: { lockKey: lockKey },
    };
  }
  current.expiresAt = now() + ttlMs;
  return openidm.update(lockPath, current._rev, current);
}

/**
 * Classify an error caught from openidm.create: true when it means "the lock _id
 * already exists" (someone holds the lock, so we should retry), false for any
 * other failure (which should propagate).
 *
 * VERIFIED in-engine on AIC IDM (sandbox, 2026-06-10) — a duplicate
 * openidm.create against managed/alpha_lock throws a Rhino-wrapped Java
 * exception with this exact shape:
 *   typeof e                              === "object"
 *   e.name                               === "JavaException"
 *   e.javaException.getClass().getName() === "org.forgerock.json.resource.PreconditionFailedException"
 *   e.message                            === "org.forgerock.json.resource.PreconditionFailedException: "
 *                                            + "Entry Already Exists: The entry "
 *                                            + "'uid=<id>,ou=alpha_lock,ou=managed,dc=openidm,dc=example,dc=com' "
 *                                            + "cannot be added because an entry with that name already exists"
 *   e.code                               === undefined (there is NO numeric code property)
 *
 * Note: a create request never carries an If-Match, so the only way a create
 * yields PreconditionFailedException is the duplicate-_id case — within this
 * usage the class match is unambiguous.
 *
 * We check the underlying Java class first (most precise; getClass() reflection
 * is permitted on IDM, unlike AM next-gen), then fall back to the two stable
 * substrings of the verified message. We do NOT match on a numeric code — there
 * isn't one.
 *
 * @param {*} e the value thrown by openidm.create
 * @returns {boolean} true if the error indicates a duplicate _id
 */
function isAlreadyExists(e) {
  if (!e) {
    return false;
  }
  // Most precise: the wrapped Java exception class (verified above).
  try {
    if (
      e.javaException &&
      e.javaException.getClass().getName() ===
        "org.forgerock.json.resource.PreconditionFailedException"
    ) {
      return true;
    }
  } catch (ignore) {
    // .javaException absent or getClass() unavailable — fall through to text.
  }
  // Fallback: the two stable substrings of the verified message.
  const msg = "" + (e.message || e);
  return (
    msg.indexOf("PreconditionFailedException") !== -1 ||
    msg.indexOf("Entry Already Exists") !== -1
  );
}

/**
 * Current time as epoch milliseconds. Wrapped in one place so the time source is
 * easy to swap if needed. Date.now() verified working on AIC IDM 2026-06-10.
 *
 * @returns {number} milliseconds since the Unix epoch
 */
function now() {
  return Date.now();
}

/**
 * Mint a per-acquisition fencing token, unique enough to distinguish lock holders:
 * the CREST transactionId (when present) plus the current time and a random suffix.
 *
 * @returns {string} an opaque owner token
 */
function newOwnerToken() {
  let txn = "";
  try {
    txn = context && context.transactionId ? context.transactionId + "-" : "";
  } catch (ignore) {}
  return txn + now() + "-" + Math.floor(Math.random() * 1e12);
}

/**
 * Pause the current thread for the given duration. java.lang.Thread.sleep is
 * verified working on AIC IDM (2026-06-10); blocks only this request thread.
 *
 * @param {number} ms milliseconds to sleep
 * @returns {void}
 */
function sleep(ms) {
  java.lang.Thread.sleep(ms);
}

/* ===========================================================================
 * EXAMPLE: a scripted-endpoint handler that runs reconById under the lock.
 * The try/finally lives inside withLock(); the handler just calls it.
 * Wire this into endpoint/<name> (or a managed-object hook) as appropriate.
 * =========================================================================== */
// (function () {
//   if (request.method !== 'action' || request.action !== 'reconById') {
//     throw { code: 400, message: 'POST ?_action=reconById with { mapping, objectId }' };
//   }
//   const mapping = request.content.mapping;
//   const objectId = request.content.objectId;
//
//   // ttlMs must exceed the worst-case reconById duration (single action call can't renew mid-flight).
//   return withLock(mapping + '-' + objectId, { ttlMs: 60000, maxRetries: 9, retryPauseMs: 2000 }, function (renew) {
//     return openidm.action('recon', 'reconById', {}, {
//       mapping: mapping,
//       ids: objectId
//     });
//   });
// })();
