var username = nodeState.get("username");
if (username === "alice") {
  action.goTo("true");
} else {
  action.goTo("false");
}
