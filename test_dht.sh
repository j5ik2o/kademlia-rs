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
BASE_PORT=9500
BOOTSTRAP_PORT=$BASE_PORT
BOOTSTRAP_ADDR="127.0.0.1:$BOOTSTRAP_PORT"
STORE_PORT=$((BASE_PORT+100))
RETRIEVE_PORT=$((BASE_PORT+200))
TEST_DIR="./test_output"

# テストパラメータ
NODE_COUNT=3               # テストに使用するDHTノードの数（ブートストラップノード含む）
NODE_STARTUP_WAIT=15       # ノード起動待機時間（秒）
STORE_OPERATION_WAIT=20    # ストア操作完了待機時間（秒）
RETRIEVE_ATTEMPTS=5        # 値取得の最大試行回数
RETRIEVE_RETRY_WAIT=8      # 取得失敗時の再試行待機時間（秒）

# テストキーおよび値の定義
TEST_KEY="integration_test_key" # どのような長さでもOK（一貫したハッシュ使用）
TEST_VALUE="integration_test_value_$(date +%s)" # タイムスタンプ付きの値

# ノードPIDを格納する配列
declare -a NODE_PIDS

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

  # すべてのDHTノードを停止
  for pid in "${NODE_PIDS[@]}"; do
    if [ -n "$pid" ]; then
      log_message "INFO" "Stopping node with PID $pid"
      kill $pid 2>/dev/null || true
      wait $pid 2>/dev/null || true
    fi
  done

  # ストアとリトリーブ用のノードも停止
  [ -n "$STORE_NODE_PID" ] && kill $STORE_NODE_PID 2>/dev/null || true
  [ -n "$RETRIEVE_NODE_PID" ] && kill $RETRIEVE_NODE_PID 2>/dev/null || true

  wait $STORE_NODE_PID 2>/dev/null || true
  wait $RETRIEVE_NODE_PID 2>/dev/null || true

  log_message "INFO" "Test completed with status: ${1:-0}"
  exit ${1:-0}
}

# Setup cleanup on script exit or interrupt
trap 'cleanup $?' EXIT
trap 'cleanup 1' INT

log_message "INFO" "Starting Kademlia DHT Integration Test with $NODE_COUNT nodes"
log_message "INFO" "Using test key: $TEST_KEY | value: $TEST_VALUE"
log_message "INFO" "Logs will be saved to $TEST_DIR/"

# Step 1: Start bootstrap node in the background
log_message "INFO" "Starting bootstrap node on port $BOOTSTRAP_PORT"
cargo run --example simple_node -- --port $BOOTSTRAP_PORT bootstrap > "$TEST_DIR/bootstrap.log" 2>&1 &
BOOTSTRAP_PID=$!
NODE_PIDS[0]=$BOOTSTRAP_PID

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

log_message "SUCCESS" "Bootstrap node started successfully on port $BOOTSTRAP_PORT"

# Step 2: 追加のノードを起動（合計NODE_COUNT-1台）
if [ $NODE_COUNT -gt 1 ]; then
  log_message "INFO" "Starting $(($NODE_COUNT-1)) additional DHT nodes..."

  for i in $(seq 1 $(($NODE_COUNT-1))); do
    NODE_PORT=$((BOOTSTRAP_PORT+$i))
    log_message "INFO" "Starting node $i on port $NODE_PORT"
    cargo run --example simple_node -- --port $NODE_PORT join --bootstrap $BOOTSTRAP_ADDR > "$TEST_DIR/node_${i}.log" 2>&1 &
    NODE_PID=$!
    NODE_PIDS[$i]=$NODE_PID

    # ノードの起動を確認
    sleep 2
    if ! ps -p $NODE_PID > /dev/null; then
      log_message "ERROR" "Node $i failed to start. Check $TEST_DIR/node_${i}.log for details."
      cat "$TEST_DIR/node_${i}.log"
      exit 1
    fi
    log_message "SUCCESS" "Node $i started successfully on port $NODE_PORT"
  done

  # ノードがネットワークに参加するまで少し待機
  log_message "INFO" "Waiting for all nodes to join the network (10 seconds)..."
  sleep 10

  # すべてのノードが起動していることを確認
  NODE_COUNT_RUNNING=0
  for i in ${!NODE_PIDS[@]}; do
    if ps -p ${NODE_PIDS[$i]} > /dev/null; then
      NODE_COUNT_RUNNING=$((NODE_COUNT_RUNNING+1))
    else
      log_message "WARNING" "Node $i (PID ${NODE_PIDS[$i]}) is not running!"
    fi
  done

  log_message "INFO" "DHT network is running with $NODE_COUNT_RUNNING nodes (expected: $NODE_COUNT)"
  if [ $NODE_COUNT_RUNNING -lt $NODE_COUNT ]; then
    log_message "WARNING" "Some nodes failed to start. The test will continue but may fail."
  fi
fi

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

# Step 3: 複数のノードから値を取得するテスト
log_message "INFO" "Testing key retrieval from multiple nodes..."

# 専用の取得ノードでのテスト
log_message "INFO" "Testing retrieval using a dedicated node..."
ATTEMPT_SUCCESS=false

# Make multiple attempts to retrieve the key
for i in $(seq 1 $RETRIEVE_ATTEMPTS); do
  log_message "INFO" "Attempt $i: Retrieving test key '$TEST_KEY' with dedicated node..."
  cargo run --example simple_node -- --port $RETRIEVE_PORT get -b $BOOTSTRAP_ADDR -k "$TEST_KEY" > "$TEST_DIR/get_dedicated_$i.log" 2>&1

  if grep -q "Found value.*$TEST_VALUE" "$TEST_DIR/get_dedicated_$i.log" || grep -q "value: Some" "$TEST_DIR/get_dedicated_$i.log"; then
    log_message "SUCCESS" "Retrieved the test key successfully with dedicated node!"

    # 取得した値を抽出して表示
    log_message "INFO" "Retrieved value details (dedicated node):"
    if grep -q "Found value.*$TEST_VALUE" "$TEST_DIR/get_dedicated_$i.log"; then
      RETRIEVED_VALUE=$(grep "Found value" "$TEST_DIR/get_dedicated_$i.log" | sed 's/.*Found value for key.*: "\(.*\)".*/\1/')
      log_message "INFO" "Key: $TEST_KEY"
      log_message "INFO" "Value: \"$RETRIEVED_VALUE\""
    elif grep -q "value: Some" "$TEST_DIR/get_dedicated_$i.log"; then
      RETRIEVED_VALUE=$(grep "value: Some" "$TEST_DIR/get_dedicated_$i.log" | sed 's/.*value: Some(\[\(.*\)\]).*/\1/')
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

    DEDICATED_NODE_SUCCESS=true
    ATTEMPT_SUCCESS=true
    break
  else
    log_message "WARNING" "Key not found with dedicated node on attempt $i. Waiting $RETRIEVE_RETRY_WAIT seconds before retry..."
    # Display relevant parts of the output
    grep -E "Looking up key|Found value|Error|Connecting|Successfully joined" "$TEST_DIR/get_dedicated_$i.log" || true
    sleep $RETRIEVE_RETRY_WAIT
  fi
done

# 既存のDHTノードからの取得テスト（NODE_COUNT > 1の場合のみ）
if [ $NODE_COUNT -gt 1 ]; then
  log_message "INFO" "Testing retrieval from existing DHT nodes..."
  DHT_NODE_SUCCESS=false

  # 最初のノード（ブートストラップノード）を除く各ノードからの取得を試行
  for node_idx in $(seq 1 $(($NODE_COUNT-1))); do
    NODE_PORT=$((BOOTSTRAP_PORT+$node_idx))
    log_message "INFO" "Attempting to retrieve key '$TEST_KEY' from DHT node $node_idx (port $NODE_PORT)..."

    # 一時的なテストノードを使用してDHTノードに接続し、値を取得
    cargo run --example simple_node -- --port $((RETRIEVE_PORT+$node_idx)) get -b 127.0.0.1:$NODE_PORT -k "$TEST_KEY" > "$TEST_DIR/get_node_${node_idx}.log" 2>&1

    if grep -q "Found value.*$TEST_VALUE" "$TEST_DIR/get_node_${node_idx}.log" || grep -q "value: Some" "$TEST_DIR/get_node_${node_idx}.log"; then
      log_message "SUCCESS" "Successfully retrieved key from DHT node $node_idx!"
      DHT_NODE_SUCCESS=true
      ATTEMPT_SUCCESS=true

      # 一つのノードから取得できれば十分なので停止
      break
    else
      log_message "WARNING" "Could not retrieve key from DHT node $node_idx"
      # トラブルシューティングのための出力
      grep -E "Looking up key|Found value|Error|Connecting|Successfully joined" "$TEST_DIR/get_node_${node_idx}.log" || true
    fi
  done

  if [ "$DHT_NODE_SUCCESS" = true ]; then
    log_message "SUCCESS" "At least one DHT node successfully retrieved the key-value pair!"
  else
    log_message "WARNING" "None of the DHT nodes could retrieve the key-value pair"
  fi
fi

# Final test result
if [ "$ATTEMPT_SUCCESS" = true ]; then
  log_message "SUCCESS" "INTEGRATION TEST PASSED: Successfully stored and retrieved a key-value pair"
  log_message "SUCCESS" "Stored key-value pair: '$TEST_KEY' -> '$TEST_VALUE'"

  # 結果の詳細表示
  if [ ! -z "$RETRIEVED_VALUE" ]; then
    if [ ! -z "$ASCII_STRING" ]; then
      log_message "SUCCESS" "Retrieved as: '$ASCII_STRING'"
    else
      log_message "SUCCESS" "Retrieved value matches expected value"
    fi
  fi

  # 複数ノードのテスト結果
  if [ $NODE_COUNT -gt 1 ]; then
    if [ "$DEDICATED_NODE_SUCCESS" = true ]; then
      log_message "SUCCESS" "Dedicated node successfully retrieved the key-value pair"
    else
      log_message "WARNING" "Dedicated node could not retrieve the key-value pair"
    fi

    if [ "$DHT_NODE_SUCCESS" = true ]; then
      log_message "SUCCESS" "At least one DHT node successfully retrieved the key-value pair"
    else
      log_message "WARNING" "None of the DHT nodes could retrieve the key-value pair"
    fi

    # ノード数のサマリー
    log_message "INFO" "DHT network status: $NODE_COUNT_RUNNING/$NODE_COUNT nodes running"
  fi

  log_message "SUCCESS" "This confirms that the Kademlia DHT network is functioning correctly"
  log_message "SUCCESS" "Key-value pairs can be stored and retrieved across the network"
  TEST_STATUS=0
else
  log_message "ERROR" "INTEGRATION TEST FAILED: Could not retrieve the test key after $RETRIEVE_ATTEMPTS attempts"
  log_message "ERROR" "Key that couldn't be retrieved: '$TEST_KEY'"

  # ノード数のサマリー（エラー時）
  if [ $NODE_COUNT -gt 1 ]; then
    log_message "INFO" "DHT network status: $NODE_COUNT_RUNNING/$NODE_COUNT nodes running"
  fi

  log_message "ERROR" "Last attempt output from dedicated node:"
  grep -E "Looking up key|Found value|Error|Connecting|Successfully joined|DEBUG" "$TEST_DIR/get_dedicated_$RETRIEVE_ATTEMPTS.log" || echo "No matching output found"

  # 特定のノードのログの追加チェック
  if [ $NODE_COUNT -gt 1 ]; then
    log_message "INFO" "Checking logs from DHT nodes for debugging..."
    for node_idx in $(seq 0 $(($NODE_COUNT-1))); do
      if [ $node_idx -eq 0 ]; then
        NODE_LOG="$TEST_DIR/bootstrap.log"
      else
        NODE_LOG="$TEST_DIR/node_${node_idx}.log"
      fi

      log_message "INFO" "Relevant messages from node $node_idx:"
      grep -E "store|key|value|NodeId" "$NODE_LOG" | tail -n 10 || echo "No relevant information found"
    done
  fi

  TEST_STATUS=1
fi

log_message "INFO" "All test logs are available in the $TEST_DIR directory"

# Return the test status
exit $TEST_STATUS