var user = openidm.read("managed/alpha_user/alice");
nodeState.putShared("mail", user.mail);
logger.info("loaded {} from openidm", user.userName);
action.goTo("true");
