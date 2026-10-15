# shellcheck shell=bash
# Server keys: show the node identity and rotate the admin token or the node
# signing key. Sourced by the ket command; never executed.
# shellcheck disable=SC2034,SC2154

hex_to_binary() {
  local hex=$1 escaped='' index
  for ((index = 0; index < ${#hex}; index += 2)); do escaped+="\\x${hex:index:2}"; done
  printf '%b' "$escaped"
}
binary_to_hex() { od -An -v -tx1 | tr -d ' \n'; }

base64url_to_hex() {
  local value=${1//-/+}
  value=${value//_//}
  while ((${#value} % 4)); do value+='='; done
  base64 -d <<<"$value" | binary_to_hex
}

# node_public_key_hex SEED: the Ed25519 public key for a node signing key
# (32-byte seed, unpadded Base64URL), via a PKCS#8 wrapper for OpenSSL.
node_public_key_hex() {
  local seed_hex
  seed_hex=$(base64url_to_hex "$1")
  [[ ${#seed_hex} == 64 ]] || fail 'KET_NODE_SIGNING_KEY is not a 32-byte key'
  hex_to_binary "302e020100300506032b657004220420$seed_hex" \
    | openssl pkey -inform DER -pubout -outform DER | tail -c 32 | binary_to_hex
}

# node_fingerprint PUBLIC_KEY_HEX: the fingerprint apps show for an invite,
# matching ket-invite's NodePublicKey::fingerprint.
node_fingerprint() {
  { printf 'KET-NODE-FINGERPRINT-V1'; hex_to_binary "$1"; } \
    | openssl dgst -sha256 -binary | head -c 16 | binary_to_hex \
    | tr 'a-f' 'A-F' | sed 's/..../&:/g; s/:$//'
}

restart_control_plane() {
  compose up --detach --no-build --force-recreate control-plane >/dev/null
  wait_for_ready || warn 'Ket did not become ready again; check: sudo ket doctor'
}

keys_usage() {
  cat <<'USAGE'
Usage: ket keys [show | rotate-admin-token | rotate-node-key]

  show                 The node fingerprint that apps check invites against.
  rotate-admin-token   Replace the local admin token (used only on this server).
  rotate-node-key      Replace the key that signs invites. Every invite and every
                       app profile made with the old key stops working.
USAGE
}

cmd_keys() {
  local action=${1:-show}
  case $action in
    -h|--help) keys_usage; return 0 ;;
  esac
  require_root
  require_installation
  require_commands openssl
  case $action in
    show) show_node_key ;;
    rotate-admin-token) rotate_admin_token ;;
    rotate-node-key) shift; rotate_node_key "$@" ;;
    *) keys_usage >&2; fail "unknown keys action: $action" ;;
  esac
}

show_node_key() {
  local seed public
  seed=$(env_value KET_NODE_SIGNING_KEY)
  if [[ -z $seed ]]; then
    info 'This server has no node signing key, so it issues access codes but no invites.'
    info 'Add one with: sudo ket keys rotate-node-key'
    return 0
  fi
  public=$(node_public_key_hex "$seed")
  info "Node fingerprint: $(node_fingerprint "$public")"
  info 'The Ket app shows this fingerprint when it checks an invite from this server.'
}

rotate_admin_token() {
  set_env_value KET_ADMIN_TOKEN "$(openssl rand -base64 48 | tr -d '\n')"
  restart_control_plane
  info 'The admin token was replaced. Invites and connected devices are not affected.'
}

rotate_node_key() {
  local yes=false public
  [[ ${1:-} == --yes ]] && yes=true
  if [[ -n $(env_value KET_NODE_SIGNING_KEY) ]]; then
    warn 'a new node key invalidates every invite from this server, and every app that'
    warn 'joined with an invite must be given a new invite. Only do this if the key leaked.'
    if ! $yes; then
      has_terminal || fail 'no terminal to confirm on; rerun with --yes'
      [[ $(ask "Type 'rotate' to replace the node key" '') == rotate ]] || { info 'Nothing changed.'; return 0; }
    fi
  fi
  set_env_value KET_NODE_SIGNING_KEY "$(openssl rand 32 | base64 | tr -d '\n=' | tr '+/' '-_')"
  restart_control_plane
  public=$(node_public_key_hex "$(env_value KET_NODE_SIGNING_KEY)")
  info "New node fingerprint: $(node_fingerprint "$public")"
  info 'Create new invites with: sudo ket invite'
}
