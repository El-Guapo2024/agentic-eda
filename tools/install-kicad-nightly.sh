#!/usr/bin/env bash
# Install KiCad nightly (kicad-dev-nightly PPA) and make it the `kicad-cli`
# our backend and parity tests use. Needs network access to
# ppa.launchpadcontent.net, keyserver.ubuntu.com and api.launchpad.net.
# Run from the cloud environment's setup script so new sessions have it.
set -euo pipefail
KEY=$(curl -sS "https://api.launchpad.net/1.0/~kicad/+archive/ubuntu/kicad-dev-nightly" | python3 -c "import sys,json;print(json.load(sys.stdin)['signing_key_fingerprint'])")
curl -sS "https://keyserver.ubuntu.com/pks/lookup?op=get&search=0x$KEY" | gpg --dearmor > /usr/share/keyrings/kicad-nightly.gpg
. /etc/os-release
echo "deb [signed-by=/usr/share/keyrings/kicad-nightly.gpg] https://ppa.launchpadcontent.net/kicad/kicad-dev-nightly/ubuntu ${VERSION_CODENAME} main" > /etc/apt/sources.list.d/kicad-nightly.list
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends kicad-nightly
ln -sf /usr/bin/kicad-cli-nightly /usr/local/bin/kicad-cli
kicad-cli version
