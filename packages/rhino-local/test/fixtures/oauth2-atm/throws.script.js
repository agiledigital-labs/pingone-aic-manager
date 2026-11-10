// Access-token modification conformance probe: an uncaught throw.
accessToken.setField("zz_before", "yes");
throw new Error("zz deliberate");
