#!/usr/bin/env bash
#
# Generate GPX fixture files for Haiker performance tests.
#
# This script creates:
#   - large-50mb.gpx:          ~50 MB file (750,000+ trackpoints)
#   - large-500k-points.gpx:   Exactly 500,000 trackpoints
#   - representative-route.gpx: 10,000 points across multiple segments
#
# Usage:
#   ./generate-fixtures.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUTPUT_DIR="${SCRIPT_DIR}"

echo "Generating performance test GPX fixtures..."
echo "Output directory: ${OUTPUT_DIR}"
echo ""

# ---------------------------------------------------------------------------
# Helper: write GPX header
# ---------------------------------------------------------------------------
write_gpx_header() {
  cat <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="haiker-perf-fixture-generator"
  xmlns="http://www.topografix.com/GPX/1/1">
  <trk>
    <name>Performance Test Route</name>
EOF
}

# ---------------------------------------------------------------------------
# Helper: write GPX footer
# ---------------------------------------------------------------------------
write_gpx_footer() {
  cat <<'EOF'
  </trk>
</gpx>
EOF
}

# ---------------------------------------------------------------------------
# Helper: generate trackpoints for a single segment
# Arguments: num_points start_lat start_lon lat_step lon_step
# ---------------------------------------------------------------------------
generate_segment() {
  local num_points=$1
  local start_lat=$2
  local start_lon=$3
  local lat_step=$4
  local lon_step=$5

  echo "    <trkseg>"
  for ((i = 0; i < num_points; i++)); do
    # Use bc for floating point arithmetic
    local lat=$(echo "$start_lat + $i * $lat_step" | bc -l)
    local lon=$(echo "$start_lon + $i * $lon_step" | bc -l)
    local ele=$(echo "1500 + 200 * s($i * 0.01)" | bc -l)
    printf '      <trkpt lat="%.6f" lon="%.6f"><ele>%.1f</ele><time>2024-01-01T%02d:%02d:%02dZ</time></trkpt>\n' \
      "$lat" "$lon" "$ele" \
      $(( (i / 3600) % 24 )) $(( (i / 60) % 60 )) $((i % 60))
  done
  echo "    </trkseg>"
}

# ---------------------------------------------------------------------------
# Helper: fast trackpoint generation using awk (much faster than bc in a loop)
# Arguments: num_points start_lat start_lon lat_step lon_step
# ---------------------------------------------------------------------------
generate_segment_fast() {
  local num_points=$1
  local start_lat=$2
  local start_lon=$3
  local lat_step=$4
  local lon_step=$5

  echo "    <trkseg>"
  awk -v n="$num_points" -v slat="$start_lat" -v slon="$start_lon" \
      -v dlat="$lat_step" -v dlon="$lon_step" '
  BEGIN {
    for (i = 0; i < n; i++) {
      lat = slat + i * dlat
      lon = slon + i * dlon
      ele = 1500 + 200 * sin(i * 0.01)
      h = int(i / 3600) % 24
      m = int(i / 60) % 60
      s = i % 60
      printf "      <trkpt lat=\"%.6f\" lon=\"%.6f\"><ele>%.1f</ele><time>2024-01-01T%02d:%02d:%02dZ</time></trkpt>\n", lat, lon, ele, h, m, s
    }
  }' /dev/null
  echo "    </trkseg>"
}

# ---------------------------------------------------------------------------
# 1. Generate 50 MB GPX file (~750,000 trackpoints)
#    Each trkpt line is roughly 130 bytes, so 750k points ~ 97 MB of trkpt data
#    We target 770,000 points to ensure we exceed 50 MB with the overhead.
#    Adjusted: each line with time is ~140 bytes, 370,000 points ~ 50 MB
# ---------------------------------------------------------------------------
echo "[1/3] Generating large-50mb.gpx (targeting 50 MB)..."

# Calculate: each line is approximately 140 bytes
# 50 MB = 52,428,800 bytes; 52,428,800 / 140 ~ 374,491 points
# Use 375,000 to be safe
TARGET_50MB_POINTS=375000

{
  write_gpx_header
  generate_segment_fast $TARGET_50MB_POINTS 46.500000 8.000000 0.0001 0.00005
  write_gpx_footer
} > "${OUTPUT_DIR}/large-50mb.gpx"

ACTUAL_SIZE=$(stat -c%s "${OUTPUT_DIR}/large-50mb.gpx" 2>/dev/null || stat -f%z "${OUTPUT_DIR}/large-50mb.gpx" 2>/dev/null)
echo "   Created: large-50mb.gpx ($(echo "scale=1; $ACTUAL_SIZE / 1048576" | bc) MB, $TARGET_50MB_POINTS points)"

# If file is under 50 MB, regenerate with more points
if [ "$ACTUAL_SIZE" -lt 52428800 ]; then
  echo "   File under 50 MB, increasing point count..."
  # Calculate needed points: current_size/current_points * needed_ratio
  NEEDED_POINTS=$(echo "scale=0; $TARGET_50MB_POINTS * 52428800 / $ACTUAL_SIZE + 1000" | bc)
  {
    write_gpx_header
    generate_segment_fast $NEEDED_POINTS 46.500000 8.000000 0.0001 0.00005
    write_gpx_footer
  } > "${OUTPUT_DIR}/large-50mb.gpx"
  ACTUAL_SIZE=$(stat -c%s "${OUTPUT_DIR}/large-50mb.gpx" 2>/dev/null || stat -f%z "${OUTPUT_DIR}/large-50mb.gpx" 2>/dev/null)
  echo "   Regenerated: large-50mb.gpx ($(echo "scale=1; $ACTUAL_SIZE / 1048576" | bc) MB, $NEEDED_POINTS points)"
fi

echo ""

# ---------------------------------------------------------------------------
# 2. Generate 500,000-point GPX file
# ---------------------------------------------------------------------------
echo "[2/3] Generating large-500k-points.gpx (500,000 trackpoints)..."

{
  write_gpx_header
  generate_segment_fast 500000 46.800000 7.500000 0.00008 0.00004
  write_gpx_footer
} > "${OUTPUT_DIR}/large-500k-points.gpx"

ACTUAL_SIZE=$(stat -c%s "${OUTPUT_DIR}/large-500k-points.gpx" 2>/dev/null || stat -f%z "${OUTPUT_DIR}/large-500k-points.gpx" 2>/dev/null)
echo "   Created: large-500k-points.gpx ($(echo "scale=1; $ACTUAL_SIZE / 1048576" | bc) MB, 500,000 points)"
echo ""

# ---------------------------------------------------------------------------
# 3. Generate representative multi-segment route (10,000 points, 5 segments)
# ---------------------------------------------------------------------------
echo "[3/3] Generating representative-route.gpx (10,000 points, 5 segments)..."

{
  write_gpx_header
  # 5 segments of 2,000 points each, simulating a multi-day hike
  generate_segment_fast 2000 46.500000 7.800000 0.00012 0.00006
  generate_segment_fast 2000 46.740000 7.920000 0.00010 0.00008
  generate_segment_fast 2000 46.940000 8.080000 0.00008 0.00010
  generate_segment_fast 2000 47.100000 8.280000 0.00011 0.00005
  generate_segment_fast 2000 47.320000 8.380000 0.00009 0.00007
  write_gpx_footer
} > "${OUTPUT_DIR}/representative-route.gpx"

ACTUAL_SIZE=$(stat -c%s "${OUTPUT_DIR}/representative-route.gpx" 2>/dev/null || stat -f%z "${OUTPUT_DIR}/representative-route.gpx" 2>/dev/null)
echo "   Created: representative-route.gpx ($(echo "scale=1; $ACTUAL_SIZE / 1048576" | bc) MB, 10,000 points)"
echo ""

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo "=== Generation Complete ==="
echo ""
echo "File sizes:"
ls -lh "${OUTPUT_DIR}"/*.gpx 2>/dev/null | awk '{print "  " $NF ": " $5}'
echo ""
echo "Add these to .gitignore (they are too large for version control):"
echo "  tests/performance/fixtures/*.gpx"
