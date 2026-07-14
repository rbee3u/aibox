#!/usr/bin/env bash
# Claude Code status line: model, effort, dir, git branch, and context usage.
input=$(cat)

# One jq pass, one field per line (mapfile keeps empty lines, so a missing
# effort — models without the parameter — stays an empty slot, not a shift).
mapfile -t f < <(jq -r '
  .model.display_name                 // "?",
  .effort.level                       // "",
  (.workspace.current_dir // .cwd     // "."),
  (.context_window.used_percentage    // 0 | floor),
  .context_window.context_window_size // 0,
  .context_window.total_input_tokens  // 0
' <<<"$input")
model=${f[0]} effort=${f[1]} dir=${f[2]} pct=${f[3]} size=${f[4]} used=${f[5]}

# git branch, best-effort (empty and silent outside a repo)
branch=$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null)

# 10-char bar; pct is input-side %, matching context_window.used_percentage
filled=$(( pct / 10 ))
bar=
for (( i = 0; i < 10; i++ )); do
    (( i < filled )) && bar+=▓ || bar+=░
done

# Round tokens to a compact unit (e.g. 74000 -> 74k, 1000000 -> 1.0M)
human() {
    awk -v n="$1" 'BEGIN {
        if      (n >= 1000000) printf "%.1fM", n / 1000000
        else if (n >= 1000)    printf "%.0fk", n / 1000
        else                   printf "%d", n
    }'
}

tag="$model${effort:+ · $effort}"
echo "📁 ${dir##*/} | ${branch:+⎇ $branch | }[$tag] | $bar $pct% ($(human "$used")/$(human "$size"))"
