#!/bin/bash
set +e
cd $(dirname $0)
source "./utils.sh"

fixture_root="$(pwd)/fixtures/error-mode"

run_build_with_deadline() {
  local project_dir="$1"
  local output_file="$2"
  local timeout_seconds="$3"
  shift 3

  (
    cd "$project_dir" || exit 1
    rewatch clean > /dev/null 2>&1
    rewatch build "$@" > "$output_file" 2>&1
  ) &

  local pid="$!"
  local remaining="$timeout_seconds"

  while kill -0 "$pid" 2> /dev/null && [ "$remaining" -gt 0 ]; do
    sleep 1
    remaining=$((remaining - 1))
  done

  if kill -0 "$pid" 2> /dev/null; then
    kill "$pid" 2> /dev/null
    wait "$pid" 2> /dev/null
    return 124
  fi

  wait "$pid"
}

assert_status_one() {
  local actual_status="$1"
  local label="$2"
  local output_file="$3"

  if [ "$actual_status" -eq 1 ]; then
    success "$label exits with status 1"
    return
  fi

  error "$label expected exit status 1, got $actual_status"
  printf "%s\n" "--- output ---" >&2
  cat "$output_file" >&2
  exit 1
}

assert_output_contains() {
  local output_file="$1"
  local expected="$2"
  local label="$3"

  if grep -Fq "$expected" "$output_file"; then
    success "$label"
    return
  fi

  error "Expected output to contain: $expected"
  printf "%s\n" "--- output ---" >&2
  cat "$output_file" >&2
  exit 1
}

assert_output_not_contains() {
  local output_file="$1"
  local unexpected="$2"
  local label="$3"

  if grep -Fq "$unexpected" "$output_file"; then
    error "Expected output not to contain: $unexpected"
    printf "%s\n" "--- output ---" >&2
    cat "$output_file" >&2
    exit 1
  fi

  success "$label"
}

bold "Test: default build reports all schedulable compile errors and exits 1"
multiple_errors_output="$(mktemp)"
run_build_with_deadline "$fixture_root/multiple-errors" "$multiple_errors_output" 10
multiple_errors_status="$?"
assert_status_one "$multiple_errors_status" "Multiple schedulable errors" "$multiple_errors_output"
assert_output_contains "$multiple_errors_output" "FirstError.res" "First-loop compile error is present"
assert_output_contains "$multiple_errors_output" "SecondError.res" "Second-loop compile error is present"
assert_output_contains "$multiple_errors_output" "Failed to Compile" "Multiple-error build reports compile failure"
rm "$multiple_errors_output"

bold "Test: --exit-after-first-error restores early-abort behavior"
first_error_only_output="$(mktemp)"
run_build_with_deadline "$fixture_root/multiple-errors" "$first_error_only_output" 10 --exit-after-first-error
first_error_only_status="$?"
assert_status_one "$first_error_only_status" "Early-abort build" "$first_error_only_output"
assert_output_contains "$first_error_only_output" "FirstError.res" "Early-abort build reports the first compile error"
assert_output_not_contains "$first_error_only_output" "SecondError.res" "Early-abort build does not reach the second-loop error"
assert_output_contains "$first_error_only_output" "Failed to Compile" "Early-abort build reports compile failure"
rm "$first_error_only_output"

bold "Test: circular references report an error, do not hang, and exit 1"
circular_output="$(mktemp)"
run_build_with_deadline "$fixture_root/circular-reference" "$circular_output" 10
circular_status="$?"
if [ "$circular_status" -eq 124 ]; then
  error "Circular reference build timed out"
  printf "%s\n" "--- output ---" >&2
  cat "$circular_output" >&2
  exit 1
fi
assert_status_one "$circular_status" "Circular reference" "$circular_output"
assert_output_contains "$circular_output" "Found a circular dependency" "Circular dependency diagnostic is present"
assert_output_contains "$circular_output" "CycleA.res" "Circular dependency output includes CycleA.res"
assert_output_contains "$circular_output" "CycleB.res" "Circular dependency output includes CycleB.res"
assert_output_contains "$circular_output" "Failed to Compile" "Circular-reference build reports compile failure"
rm "$circular_output"
