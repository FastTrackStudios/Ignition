#!/usr/bin/env bash
# The frame breakdown of docs/lighting/research/frame-breakdown.md, measured.
#
#   features/lighting/tools/frame_breakdown.sh <label> [repeats] [sizes…]
#
# Run from the checkout root, in the dev shell, after
#   cargo build --release --workspace --bin viz --bin ignition
#   cargo test  --release --workspace --bin ignition --test process_view --no-run
# Needs a GPU and IGNITION_LIVE_SET. No display: every device is headless.
#
# For each size (2560x1440 and 5120x1440 unless given) and each repeat, in
# turn: the renderer with no pane (`ignition --bench`), `viz --bench` on
# the same venue and cue, the pane over the platform's GPU-shared frames,
# over shared memory, shared memory with the rendering drained before the
# copy (`IGNITION_FRAME_DRAIN=1`), and shared memory with the app's upload.
# The runs are interleaved so a change in the machine's load lands on
# every mode alike. One line per run goes to
# target/tmp/frame-breakdown/<label>.tsv, with the load average before it.
set -euo pipefail

label="${1:?a label for this run (before, after, …)}"
repeats="${2:-3}"
shift $(( $# > 2 ? 2 : $# ))
sizes=("$@")
[ ${#sizes[@]} -gt 0 ] || sizes=(2560x1440 5120x1440)

: "${IGNITION_LIVE_SET:?the Live set (.als)}"
root="$(git rev-parse --show-toplevel)"
out="$root/target/tmp/frame-breakdown"
mkdir -p "$out"
tsv="$out/$label.tsv"
frames="${FRAMES:-300}"
# The chorus of song 8 (Figure It Out), the clock running.
show=(--start 8 --bar 31 --go)
ignition="$root/target/release/ignition"
viz="$root/target/release/viz"
pane="$(ls -t "$root"/target/release/deps/process_view-* | grep -v '\.d$' | head -1)"

plain() { sed 's/\x1b\[[0-9;]*m//g'; }
# The `ignition::frames` event's fields as `key=value` words.
summary() { plain | { grep "ignition::frames" || true; } | tail -1 | sed 's/.*ignition::frames: frames //'; }
field() { tr ' ' '\n' | sed -n "s/^$1=//p" | tr -d '"' | head -1; }

printf 'mode\tsize\trun\tload_before\tload_during\tupdate_p50\tupdate_p99\tdrain_p50\tsubmit_p50\twait_p50\twait_p99\tmemcpy_p50\tmemcpy_p99\tsend_p50\ttotal_p50\ttotal_p99\tinterval_p50\tinterval_p99\tfps\tnote\n' > "$tsv"

row() { # mode size run load_before summary-text fps note
    local s="$5"
    local f
    f() { printf '%s' "$s" | field "$1"; }
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$1" "$2" "$3" "$4" "$(f load1)" \
        "$(f update_p50_us)" "$(f update_p99_us)" "$(f drain_p50_us)" "$(f submit_p50_us)" \
        "$(f wait_p50_us)" "$(f wait_p99_us)" "$(f memcpy_p50_us)" "$(f memcpy_p99_us)" \
        "$(f send_p50_us)" "$(f total_p50_us)" "$(f total_p99_us)" \
        "$(f interval_p50_us)" "$(f interval_p99_us)" "$6" "$7" >> "$tsv"
}

load() { cut -d' ' -f1 /proc/loadavg; }

as_pane() { # size transport-env drain upload → log
    local size="$1" transport="$2" drain="$3" upload="$4"
    ( cd "$root/apps/ignition" && \
      env ${transport:+IGNITION_TRANSPORT=$transport} IGNITION_FRAME_DRAIN="$drain" \
          IGNITION_HOST_UPLOAD="$upload" IGNITION_PANE_SIZE="$size" \
          IGNITION_PANE_FRAMES="$frames" IGNITION_FRAME_STATS="$frames" \
          IGNITION_PANE_ARGS="--bar 31 --go" \
          timeout 600 "$pane" --ignored --nocapture the_renderers_frame_time_as_a_pane 2>&1 ) || true
}

for run in $(seq 1 "$repeats"); do
  for size in "${sizes[@]}"; do
    w="${size%x*}"; h="${size#*x}"

    before="$(load)"
    log="$(cd "$root" && timeout 600 "$ignition" "${show[@]}" --bench "$frames" --size "$size" </dev/null 2>&1 || true)"
    row none "$size" "$run" "$before" "$(printf '%s' "$log" | summary)" "" "ignition --bench"

    before="$(load)"
    log="$(cd "$root/features/lighting" && timeout 600 "$viz" --venue data/venues/room138-cbu \
        --cuelist data/songs/jt-recital/figure-it-out.json --cue 5 \
        --bench "$frames" --width "$w" --height "$h" 2>&1 | plain || true)"
    line="$(printf '%s' "$log" | grep '^bench [0-9]' | tail -1 || true)"
    printf 'viz-bench\t%s\t%s\t%s\t%s\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t%s\n' "$size" "$run" "$before" "$(load)" "$line" >> "$tsv"

    for mode in gpu shm shm-drain shm-upload; do
      case "$mode" in
        gpu)        t="";    d=0; u=0 ;;
        shm)        t="shm"; d=0; u=0 ;;
        shm-drain)  t="shm"; d=1; u=0 ;;
        shm-upload) t="shm"; d=0; u=1 ;;
      esac
      before="$(load)"
      log="$(as_pane "$size" "$t" "$d" "$u")"
      pane_line="$(printf '%s' "$log" | plain | grep '^renderer as a pane' | tail -1 || true)"
      upload_line="$(printf '%s' "$log" | plain | grep "^the app's upload" | tail -1 | sed 's/.*: call/upload call/' || true)"
      fps="$(printf '%s' "$pane_line" | sed -n 's/.*; \([0-9.]*\) frames\/s.*/\1/p')"
      transport="$(printf '%s' "$pane_line" | sed -n 's/.*(\([A-Za-z]*\),.*/\1/p')"
      row "$mode" "$size" "$run" "$before" "$(printf '%s' "$log" | summary)" "$fps" "$transport ${upload_line}"
    done
  done
done
echo "wrote $tsv"
