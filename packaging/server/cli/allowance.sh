# shellcheck shell=bash
# Data allowances on invites: reading "50GB/month" and printing what an
# invite's devices have used. Sizes are binary (1 GB is 1024 MB), as Ket
# prints traffic everywhere. Sourced by the ket command and create-invite.sh;
# never executed.

# allowance_json TEXT: prints TEXT ("50GB/month", "2GB/day", "1.5TB/week", or
# "none") as the API's JSON: {"bytes":N,"period":"month"}, or null for none.
allowance_json() {
  local text=${1,,} whole fraction unit period scale bytes
  text=${text// /}
  if [[ $text == none ]]; then
    printf 'null'
    return 0
  fi
  if ! [[ $text =~ ^([0-9]{1,7})(\.([0-9]{1,3}))?(mb|gb|tb)/(day|week|month)$ ]] \
    || ((10#${BASH_REMATCH[1]} > 1048576)); then
    printf '"%s" is not a data allowance; use a size and a period, such as 50GB/month, 2GB/day, or none\n' "$1" >&2
    return 1
  fi
  whole=${BASH_REMATCH[1]} fraction=${BASH_REMATCH[3]} unit=${BASH_REMATCH[4]} period=${BASH_REMATCH[5]}
  case $unit in
    mb) scale=$((1 << 20)) ;;
    gb) scale=$((1 << 30)) ;;
    *) scale=$((1 << 40)) ;;
  esac
  bytes=$((10#$whole * scale))
  [[ -z $fraction ]] || bytes=$((bytes + 10#$fraction * scale / 10 ** ${#fraction}))
  if ((bytes < 1 << 20 || bytes > 1 << 50)); then
    printf 'a data allowance must be from 1MB to 1024TB\n' >&2
    return 1
  fi
  printf '{"bytes":%s,"period":"%s"}' "$bytes" "$period"
}

# jq definitions: `size` prints a byte count; on an allowance status,
# `allowance_short` fits a table column and `allowance_line` a sentence.
# shellcheck disable=SC2016 # jq, not shell, expands these.
ALLOWANCE_JQ='def size: if . >= 1099511627776 then "\(. / 1099511627776 * 10 | floor / 10) TB"
    elif . >= 1073741824 then "\(. / 1073741824 * 10 | floor / 10) GB"
    elif . >= 1048576 then "\(. / 1048576 * 10 | floor / 10) MB"
    elif . >= 1024 then "\(. / 1024 | floor) KB" else "\(.) B" end;
  def each: {day: "a day", week: "a week", month: "a month"}[.period] // .period;
  def this: {day: "today", week: "this week", month: "this month"}[.period] // "this period";
  def allowance_short: if . == null then "-"
    elif (.used_bytes // 0) >= .bytes then "used up"
    else "\(.used_bytes // 0 | size)/\(.bytes | size)" end;
  def allowance_line: if . == null then "none"
    else "\(.used_bytes // 0 | size) of \(.bytes | size) used \(this) (\(.bytes | size) \(each),"
      + " shared by its devices"
      + (.resets_at_epoch_seconds // null
         | if . == null then "" else "; starts again \(strftime("%Y-%m-%d %H:%M UTC"))" end)
      + ")"
    end;'

# allowance_sentence JSON: an allowance as the API takes it, in words.
allowance_sentence() {
  jq -r "$ALLOWANCE_JQ"' if . == null then "no data allowance"
    else "\(.bytes | size) \(each), shared by its devices" end' <<<"$1"
}
