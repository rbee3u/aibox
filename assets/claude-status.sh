#!/usr/bin/env bash
# Claude Code status line: model, effort, dir, git branch, and context usage.
input=$(cat)

# One jq pass, one field per line (mapfile keeps empty lines, so a missing
# effort — models without the parameter — stays an empty slot, not a shift).
mapfile -t fields < <(jq -r '
  .model.display_name                 // "?",
  .effort.level                       // "",
  (.workspace.current_dir // .cwd     // "."),
  (.context_window.used_percentage    // 0 | floor),
  .context_window.context_window_size // 0,
  .context_window.total_input_tokens  // 0
' <<<"$input")
model=${fields[0]}
effort=${fields[1]}
workspace_dir=${fields[2]}
context_percent=${fields[3]}
context_size=${fields[4]}
input_tokens=${fields[5]}

# git branch, best-effort (empty and silent outside a repo)
branch=$(git -C "$workspace_dir" rev-parse --abbrev-ref HEAD 2>/dev/null)

# 10-char bar; the percentage is input-side context usage.
filled=$(( context_percent / 10 ))
bar=
for (( index = 0; index < 10; index++ )); do
    (( index < filled )) && bar+=▓ || bar+=░
done

# Round tokens to a compact unit (e.g. 74000 -> 74k, 1000000 -> 1.0M)
format_tokens() {
    awk -v n="$1" 'BEGIN {
        if      (n >= 1000000) printf "%.1fM", n / 1000000
        else if (n >= 1000)    printf "%.0fk", n / 1000
        else                   printf "%d", n
    }'
}

tag="$model${effort:+ · $effort}"
echo "📁 ${workspace_dir##*/} | ${branch:+⎇ $branch | }[$tag] | $bar $context_percent% ($(format_tokens "$input_tokens")/$(format_tokens "$context_size"))"
