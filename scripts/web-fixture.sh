#!/bin/sh
# A scratch pma store with three worktree runs ready for review, the third
# a bundle of two items, for the web
# checks. Everything is written under <dir>, which must not exist yet: a
# seed repository, a bare origin, a clone at <dir>/root/alpha, a fake
# `claude`, and the store at <dir>/home. Run it yourself: it commits in the
# seed repository, which an agent here may not do.
#
#   sh scripts/web-fixture.sh <dir>
#   . <dir>/env.sh && target/debug/pma web --no-open
set -e
[ -n "$1" ] || { echo "usage: $0 <dir>" >&2; exit 2; }
[ ! -e "$1" ] || { echo "$1 exists; name a new directory" >&2; exit 2; }
S="$(mkdir -p "$1" && cd "$1" && pwd)"
P="$(cd "$(dirname "$0")/.." && pwd)/target/debug/pma"
mkdir -p "$S/bin" "$S/seed" "$S/root"
cat > "$S/bin/claude" <<'EOF'
#!/bin/sh
echo hi > hello.txt
echo '{"type":"result","subtype":"success","is_error":false,"result":"Added hello.txt with a greeting.","total_cost_usd":0.1}'
EOF
chmod +x "$S/bin/claude"
# The same environment for the server, so its commits and pushes stay here.
cat > "$S/env.sh" <<EOF
export PMA_HOME="$S/home" PATH="$S/bin:\$PATH" GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=u GIT_AUTHOR_EMAIL=u@example.com GIT_COMMITTER_NAME=u GIT_COMMITTER_EMAIL=u@example.com
EOF
. "$S/env.sh"
cd "$S/seed"
git init -q -b main
printf '# TODO\n\n## High\n\n- [ ] add greeting #agent\n  say hello\n- [ ] second task #agent\n- [ ] third task #agent\n- [ ] fourth task #agent\n' > TODO.md
printf 'test:\n\ttest -f hello.txt\n' > Makefile
git add . && git commit -qm init
cd "$S" && git clone -q --bare seed origin.git
cd "$S/root" && git clone -q ../origin.git alpha
"$P" root add "$S/root" >/dev/null
"$P" project tier 1 alpha >/dev/null
"$P" config workspace worktree >/dev/null
"$P" scan --offline >/dev/null
"$P" dispatch alpha:5
"$P" dispatch alpha:7
"$P" dispatch alpha:8 alpha:9
"$P" review
echo "store: $S/home; run \`. $S/env.sh\` before \`pma web --no-open\`"
