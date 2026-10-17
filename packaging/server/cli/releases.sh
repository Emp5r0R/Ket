# shellcheck shell=bash
# Release lookup and update decisions for the ket server command. Sourced,
# never executed. The rating rule mirrors ReleaseManifest::server_update_category
# in crates/ket-update; both replay crates/ket-update/tests/vectors/server-updates.json.
# shellcheck disable=SC2034,SC2154

github_url() { printf '%s' "${KET_GITHUB_URL:-https://github.com}"; }
raw_url() { printf '%s' "${KET_RAW_URL:-https://raw.githubusercontent.com}"; }

# latest_release_tag: the tag GitHub marks as the latest release.
latest_release_tag() {
  local latest tag
  latest=$(curl -fsSLI --max-time 30 -o /dev/null -w '%{url_effective}' \
    "$(github_url)/$KET_REPOSITORY/releases/latest") || return 1
  tag=${latest##*/tag/}
  [[ $tag != "$latest" && $tag =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]] || return 1
  printf '%s' "$tag"
}

# release_manifest TAG: that release's releases.json, or failure for releases
# from before the update lifecycle.
release_manifest() {
  curl -fsSL --max-time 30 "$(raw_url)/$KET_REPOSITORY/$1/crates/ket-update/releases.json"
}

# update_category MANIFEST INSTALLED TARGET: prints the most urgent server or
# app rating among releases after INSTALLED up to TARGET, or nothing.
update_category() {
  jq -r --arg installed "$2" --arg target "$3" '
    def v: split(".") | map(tonumber);
    ["optional", "recommended", "security", "critical"] as $order
    | [ .releases[]
        | select((.version | v) > ($installed | v) and (.version | v) <= ($target | v))
        | (.server, .client)
        | select(. != null)
        | . as $category
        | $order | index($category) ]
    | max
    | if . == null then empty else $order[.] end
  ' <<<"$1"
}

# app_categories MANIFEST INSTALLED TARGET: prints a JSON object with the most
# urgent app rating for each platform among releases after INSTALLED up to
# TARGET, leaving out apps the update does not change. A release's apps map
# rates each app; without it, client applies to every app. Mirrors
# ReleaseManifest::app_update_category.
app_categories() {
  jq -c --arg installed "$2" --arg target "$3" '
    def v: split(".") | map(tonumber);
    ["optional", "recommended", "security", "critical"] as $order
    | [ .releases[]
        | select((.version | v) > ($installed | v) and (.version | v) <= ($target | v)) ] as $range
    | [ ["android", "linux", "macos", "windows"][] as $platform
        | [ $range[]
            | (if (.apps // {}) == {} then .client else .apps[$platform] end)
            | select(. != null)
            | . as $category
            | $order | index($category) ]
        | max
        | select(. != null)
        | {key: $platform, value: $order[.]} ]
    | from_entries
  ' <<<"$1"
}

# app_category MANIFEST INSTALLED TARGET PLATFORM: that platform's rating from
# app_categories, or nothing.
app_category() {
  app_categories "$1" "$2" "$3" | jq -r --arg platform "$4" '.[$platform] // empty'
}

# app_ratings MANIFEST INSTALLED TARGET: one line saying what each app is told
# after this update, such as "android optional, linux security", or nothing
# when the update changes no app.
app_ratings() {
  app_categories "$1" "$2" "$3" | jq -r '
    to_entries
    | if length == 0 then empty
      elif length == 4 and (map(.value) | unique | length) == 1 then "every app \(.[0].value)"
      else map("\(.key) \(.value)") | join(", ") end'
}

# auto_update_allows POLICY CATEGORY: whether KET_AUTO_UPDATE=POLICY applies
# an update rated CATEGORY (empty for an unrated update) without asking.
auto_update_allows() {
  case $1:$2 in
    all:*) return 0 ;;
    security:security | security:critical | critical:critical) return 0 ;;
    *) return 1 ;;
  esac
}

auto_update_policy() {
  local policy
  policy=$(env_value KET_AUTO_UPDATE)
  case ${policy:-security} in
    off | critical | security | all) printf '%s' "${policy:-security}" ;;
    *) fail "KET_AUTO_UPDATE must be off, critical, security, or all (found: $policy)" ;;
  esac
}

# record_update_check INSTALLED LATEST CATEGORY OUTCOME [APPS]: records the
# last update check for `ket status`. APPS is app_ratings' line.
record_update_check() {
  local installed=$1 latest=$2 category=$3 outcome=$4 apps=${5:-} dir
  dir=$(ket_state_dir)
  install -d -m 0755 "$dir"
  jq -n --arg installed "$installed" --arg latest "$latest" --arg category "$category" \
    --arg outcome "$outcome" --arg apps "$apps" --arg checked "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    '{installed:$installed, latest:$latest, category:$category, apps:$apps, outcome:$outcome, checked:$checked}' \
    > "$dir/update-check.json"
}
