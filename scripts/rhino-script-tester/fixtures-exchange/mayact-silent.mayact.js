// D6 negative case. Runs, logs, and stamps nothing.
//
// The point of the pair is that this is not an error condition: the script
// succeeds, the subject token is issued normally and looks entirely healthy.
// Only the exchange fails, with `400 invalid_request "Invalid token
// exchange."` — the same message a forged token, a denied scope and a
// malformed request all produce.
logger.error("AICEDIT-D6 may-act script ran and stamped nothing");
