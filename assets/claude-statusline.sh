#!/usr/bin/env bash
# Claude Code statusline: model/reasoning, directory, branch, and context
# size/usage.
# `jq` provides the payload fields; the Git branch is best-effort.
input=$(cat)

empty_marker=__AIBOX_STATUSLINE_EMPTY__
IFS=$'\t' read -r model effort workspace_dir context_size context_percent < <(
    jq -r '[
      (.model.display_name // "__AIBOX_STATUSLINE_EMPTY__"),
      (.effort.level // "__AIBOX_STATUSLINE_EMPTY__"),
      (.workspace.current_dir // .cwd // "__AIBOX_STATUSLINE_EMPTY__"),
      (.context_window.context_window_size // "__AIBOX_STATUSLINE_EMPTY__"),
      (.context_window.used_percentage // "__AIBOX_STATUSLINE_EMPTY__")
      ] | @tsv' <<<"$input"
)
[[ "$model" == "$empty_marker" ]] && model=
[[ "$effort" == "$empty_marker" ]] && effort=
[[ "$workspace_dir" == "$empty_marker" ]] && workspace_dir=
[[ "$context_size" == "$empty_marker" ]] && context_size=
[[ "$context_percent" == "$empty_marker" ]] && context_percent=

# Use decimal units and three significant digits, matching the compact form in
# the native Codex statusline (for example, 258000 -> 258K).
format_tokens() {
    awk -v raw="$1" 'BEGIN {
        if (raw == "" || raw !~ /^[0-9]+([.][0-9]+)?$/) exit
        n = raw + 0
        if (n < 1000) {
            printf "%.0f", n
            exit
        } else if (n < 999500) {
            scaled = n / 1000
            suffix = "K"
        } else {
            scaled = n / 1000000
            suffix = "M"
        }
        if (scaled >= 100) formatted = sprintf("%.0f", scaled)
        else if (scaled >= 10) formatted = sprintf("%.1f", scaled)
        else formatted = sprintf("%.2f", scaled)
        if (index(formatted, ".") > 0) {
            sub(/0+$/, "", formatted)
            sub(/\.$/, "", formatted)
        }
        printf "%s%s", formatted, suffix
    }'
}

format_context_used() {
    awk -v raw="$1" 'BEGIN {
        if (raw == "" || raw !~ /^-?[0-9]+([.][0-9]+)?$/) exit
        n = raw + 0
        if (n < 0) n = 0
        else if (n > 100) n = 100
        printf "Context %d%% used", int(n)
    }'
}

display_dir=$workspace_dir
if [[ -n "${HOME:-}" ]]; then
    case "$workspace_dir" in
        "$HOME") display_dir="~" ;;
        "$HOME"/*) display_dir="~${workspace_dir#"$HOME"}" ;;
    esac
fi

# Detached HEAD and non-repository directories intentionally have no branch.
branch=
if [[ -n "$workspace_dir" ]]; then
    branch=$(git -C "$workspace_dir" symbolic-ref --quiet --short HEAD 2>/dev/null || true)
fi

if [[ -n "$model" && -n "$effort" ]]; then
    model_with_reasoning="$model $effort"
else
    model_with_reasoning="$model$effort"
fi

segments=()
add_segment() {
    local value="$1"
    local label="$2"
    [[ -n "$value" ]] || return 0
    segments+=("${value}${label:+ $label}")
}

add_segment "$model_with_reasoning" ""
add_segment "$display_dir" ""
add_segment "$branch" ""
add_segment "$(format_tokens "$context_size")" "window"
add_segment "$(format_context_used "$context_percent")" ""

output=
for segment in "${segments[@]}"; do
    [[ -n "$output" ]] && output+=" · "
    output+="$segment"
done
printf '%s\n' "$output"
