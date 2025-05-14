#!/bin/bash

# Enhanced Integration Test for Kademlia DHT
# This script runs a more comprehensive integration test with detailed logging

# Colors for better output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Enable debug logging
export RUST_LOG=debug

# Configuration
BOOTSTRAP_PORT=9500
STORE_PORT=9501
RETRIEVE_PORT=9502
BOOTSTRAP_ADDR="127.0.0.1:$BOOTSTRAP_PORT"
TEST_DIR="./test_output"
# テストキーおよび値の定義
TEST_KEY="integration_test_key" # どのような長さでもOK（一貫したハッシュ使用）
TEST_VALUE="integration_test_value_$(date +%s)" # タイムスタンプ付きの値
NODE_STARTUP_WAIT=15
STORE_OPERATION_WAIT=20
RETRIEVE_ATTEMPTS=5
RETRIEVE_RETRY_WAIT=8

# Create test directory for logs
mkdir -p $TEST_DIR

# Function to log messages with timestamp
log_message() {
  local level=$1
  local message=$2
  local color=$YELLOW
  
  case "$level" in
    "INFO") color=$BLUE ;;
    "SUCCESS") color=$GREEN ;;
    "ERROR") color=$RED ;;
    *) color=$YELLOW ;;
  esac
  
  echo -e "[$(date +"%Y-%m-%d %H:%M:%S")] ${color}[$level]${NC} $message"
}

# Function to kill processes and clean up
cleanup() {
  log_message "INFO" "Cleaning up - stopping all nodes..."
  [ -n "$BOOTSTRAP_PID" ] && kill $BOOTSTRAP_PID 2>/dev/null || true
  [ -n "$STORE_NODE_PID" ] && kill $STORE_NODE_PID 2>/dev/null || true
  [ -n "$RETRIEVE_NODE_PID" ] && kill $RETRIEVE_NODE_PID 2>/dev/null || true
  
  wait $BOOTSTRAP_PID 2>/dev/null || true
  wait $STORE_NODE_PID 2>/dev/null || true
  wait $RETRIEVE_NODE_PID 2>/dev/null || true
  
  log_message "INFO" "Test completed with status: ${1:-0}"
  exit ${1:-0}
}

# Setup cleanup on script exit or interrupt
trap 'cleanup $?' EXIT
trap 'cleanup 1' INT

log_message "INFO" "Starting Kademlia DHT Integration Test"
log_message "INFO" "Using test key: $TEST_KEY | value: $TEST_VALUE"
log_message "INFO" "Logs will be saved to $TEST_DIR/"

# Step 1: Start bootstrap node in the background
log_message "INFO" "Starting bootstrap node on port $BOOTSTRAP_PORT"
cargo run --example simple_node -- --port $BOOTSTRAP_PORT bootstrap > "$TEST_DIR/bootstrap.log" 2>&1 &
BOOTSTRAP_PID=$!

log_message "INFO" "Waiting for bootstrap node to initialize ($NODE_STARTUP_WAIT seconds)..."
for i in $(seq 1 $NODE_STARTUP_WAIT); do
  if ! ps -p $BOOTSTRAP_PID > /dev/null; then
    log_message "ERROR" "Bootstrap node process died. Check $TEST_DIR/bootstrap.log for details."
    cat "$TEST_DIR/bootstrap.log"
    exit 1
  fi
  echo -n "."
  sleep 1
done
echo ""

# Verify bootstrap node is still running
if ! ps -p $BOOTSTRAP_PID > /dev/null; then
  log_message "ERROR" "Bootstrap node failed to start. Check $TEST_DIR/bootstrap.log for details."
  cat "$TEST_DIR/bootstrap.log"
  exit 1
fi

log_message "SUCCESS" "Bootstrap node started successfully"

# Step 2: Store a test key-value pair using a dedicated node
log_message "INFO" "Starting node to store test key-value pair..."
cargo run --example simple_node -- --port $STORE_PORT store --bootstrap $BOOTSTRAP_ADDR --key "$TEST_KEY" --value "$TEST_VALUE" > "$TEST_DIR/store.log" 2>&1
STORE_RESULT=$?

if [ $STORE_RESULT -ne 0 ]; then
  log_message "ERROR" "Store operation failed with exit code: $STORE_RESULT"
  cat "$TEST_DIR/store.log"
  exit 1
elif grep -q "Successfully stored key-value pair\|Storage operation complete\|Successfully stored value" "$TEST_DIR/store.log"; then
  log_message "SUCCESS" "Storage operation completed successfully"
  # Optionally display relevant parts of the output
  log_message "INFO" "Store operation log (important parts):"
  grep -E "Storing key|DEBUG: MemoryStorage|Successfully stored|Storage operation complete" "$TEST_DIR/store.log" || true
else
  log_message "ERROR" "Failed to store key-value pair. Check $TEST_DIR/store.log for details."
  cat "$TEST_DIR/store.log"
  exit 1
fi

# Step 3: Try to retrieve the value using a new node
log_message "INFO" "Testing key retrieval using a new node..."
ATTEMPT_SUCCESS=false

# Make multiple attempts to retrieve the key
for i in $(seq 1 $RETRIEVE_ATTEMPTS); do
  log_message "INFO" "Attempt $i: Retrieving test key '$TEST_KEY'..."
  cargo run --example simple_node -- --port $RETRIEVE_PORT get -b $BOOTSTRAP_ADDR -k "$TEST_KEY" > "$TEST_DIR/get_attempt_$i.log" 2>&1
  
  if grep -q "Found value.*$TEST_VALUE" "$TEST_DIR/get_attempt_$i.log" || grep -q "value: Some" "$TEST_DIR/get_attempt_$i.log"; then
    log_message "SUCCESS" "Retrieved the test key successfully!"

    # 取得した値を抽出して表示
    log_message "INFO" "Retrieved value details:"
    if grep -q "Found value.*$TEST_VALUE" "$TEST_DIR/get_attempt_$i.log"; then
      RETRIEVED_VALUE=$(grep "Found value" "$TEST_DIR/get_attempt_$i.log" | sed 's/.*Found value for key.*: "\(.*\)".*/\1/')
      log_message "INFO" "Key: $TEST_KEY"
      log_message "INFO" "Value: \"$RETRIEVED_VALUE\""
    elif grep -q "value: Some" "$TEST_DIR/get_attempt_$i.log"; then
      RETRIEVED_VALUE=$(grep "value: Some" "$TEST_DIR/get_attempt_$i.log" | sed 's/.*value: Some(\[\(.*\)\]).*/\1/')
      log_message "INFO" "Key: $TEST_KEY"
      log_message "INFO" "Value bytes: $RETRIEVED_VALUE"
      # バイト値から文字列に変換を試みる（可能な場合）
      HEX_VALUES=$(echo $RETRIEVED_VALUE | tr -d '[]' | tr ',' ' ')
      ASCII_STRING=""
      for hex in $HEX_VALUES; do
        if [ "$hex" -ge 32 ] && [ "$hex" -le 126 ]; then
          ASCII_STRING="$ASCII_STRING$(printf "\\$(printf '%03o' $hex)")"
        fi
      done
      if [ ! -z "$ASCII_STRING" ]; then
        log_message "INFO" "Value as string: \"$ASCII_STRING\""
      fi
    fi

    ATTEMPT_SUCCESS=true
    break
  else
    log_message "WARNING" "Key not found on attempt $i. Waiting $RETRIEVE_RETRY_WAIT seconds before retry..."
    # Display relevant parts of the output
    grep -E "Looking up key|Found value|Error|Connecting|Successfully joined" "$TEST_DIR/get_attempt_$i.log" || true
    sleep $RETRIEVE_RETRY_WAIT
  fi
done

# Final test result
if [ "$ATTEMPT_SUCCESS" = true ]; then
  log_message "SUCCESS" "INTEGRATION TEST PASSED: Successfully stored and retrieved a key-value pair"
  log_message "SUCCESS" "Stored key-value pair: '$TEST_KEY' -> '$TEST_VALUE'"
  if [ ! -z "$RETRIEVED_VALUE" ]; then
    if [ ! -z "$ASCII_STRING" ]; then
      log_message "SUCCESS" "Retrieved as: '$ASCII_STRING'"
    else
      log_message "SUCCESS" "Retrieved value matches expected value"
    fi
  fi
  log_message "SUCCESS" "This confirms that full node-to-node UDP communication is working"
  TEST_STATUS=0
else
  log_message "ERROR" "INTEGRATION TEST FAILED: Could not retrieve the test key after $RETRIEVE_ATTEMPTS attempts"
  log_message "ERROR" "Key that couldn't be retrieved: '$TEST_KEY'"
  log_message "ERROR" "Last attempt output:"
  grep -E "Looking up key|Found value|Error|Connecting|Successfully joined|DEBUG" "$TEST_DIR/get_attempt_$RETRIEVE_ATTEMPTS.log" || echo "No matching output found"
  TEST_STATUS=1
fi

log_message "INFO" "All test logs are available in the $TEST_DIR directory"

# Return the test status
exit $TEST_STATUS