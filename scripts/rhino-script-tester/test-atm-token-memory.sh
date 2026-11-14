#!/usr/bin/env bash
# Exercise the real helper without credentials, network, or tenant writes.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
python3 - "$SCRIPT_DIR/run-atm-probes.sh" <<'PY'
import base64
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

helper = Path(sys.argv[1])
with tempfile.TemporaryDirectory(prefix="aic-atm-memory-") as directory:
    root = Path(directory)
    bindir = root / "bin"
    bindir.mkdir()
    scratch = root / "tmp"
    scratch.mkdir()
    payload = base64.urlsafe_b64encode(json.dumps({
        "aicprobe": {"ok": True}, "aud": "synthetic-client",
        "client_id": "synthetic-client",
    }).encode()).decode().rstrip("=")
    access = "synthetic-header." + payload + ".synthetic-signature"
    refresh = "synthetic-" + "refresh-marker"
    response = json.dumps({"access_token": access, "refresh_token": refresh})
    stubs = {
        "aic": "#!/usr/bin/env bash\nexit 0\n",
        "sleep": "#!/usr/bin/env bash\nexit 0\n",
        "curl": '''#!/usr/bin/env python3
import os
from pathlib import Path
import sys
args = sys.argv[1:]
body = os.environ["SYNTHETIC_RESPONSE"]
output = None
for i, arg in enumerate(args):
    if arg in ("-o", "--output"):
        output = args[i + 1]
if output:
    Path(output).write_text(body)
else:
    sys.stdout.write(body)
sys.stdout.flush()
for path in Path(os.environ["CHECK_ROOT"]).rglob("*"):
    if path.is_file() and any(token.encode() in path.read_bytes()
        for token in (os.environ["ACCESS_MARKER"], os.environ["REFRESH_MARKER"])):
        Path(os.environ["CHECK_ROOT"], "violation").touch()
sys.stdout.write("\\n200\\n" if not output else "200")
''',
    }
    for name, source in stubs.items():
        path = bindir / name
        path.write_text(source)
        path.chmod(0o755)
    fixture = root / "check.script.js"
    fixture.write_text("// synthetic fixture\n")
    secret = root / "secret"
    secret.write_text("synthetic-secret")
    env = dict(os.environ, PATH=str(bindir) + ":" + os.environ["PATH"],
        TMPDIR=str(scratch), BASE="https://tenant.example.com",
        AIC_BIN=str(bindir / "aic"), WORKSPACE_CJS=str(root / "workspace.cjs"),
        OUT_DIR=str(root / "out"), SECRET_FILE=str(secret), PASSWORD_FILE=str(secret),
        USERNAME="synthetic-user", CLIENT_ID="synthetic-client", CHECK_ROOT=str(root),
        SYNTHETIC_RESPONSE=response, ACCESS_MARKER=access, REFRESH_MARKER=refresh)
    for grant in ("client_credentials", "password"):
        env["GRANT_TYPE"] = grant
        subprocess.run(["bash", str(helper), str(fixture)], env=env,
            check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        assert not (root / "violation").exists(), "token response written to disk"
        rows = json.loads((root / "out/atm-probe-results.json").read_text())
        assert rows[0]["http"] == "200", rows
        assert rows[0]["claim"] == {"ok": True}, rows
        assert rows[0]["aud"] == "string:equals-client-id", rows
        for path in root.rglob("*"):
            if path.is_file():
                assert access.encode() not in path.read_bytes(), path
                assert refresh.encode() not in path.read_bytes(), path
print("ATM helper: both grants keep tokens off disk and retain sanitized results")
PY
