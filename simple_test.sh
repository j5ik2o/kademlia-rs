#!/bin/bash

# Simple local test script for Kademlia DHT
# This script tests basic communication between a bootstrap node and a client

# Colors for better output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

# Configuration
BOOTSTRAP_PORT=9800
CLIENT_PORT=9801
export RUST_LOG=debug

echo -e "${YELLOW}Starting Kademlia Simple Test...${NC}"

# Function to kill processes and clean up
cleanup() {
  echo -e "${YELLOW}Cleaning up - stopping all nodes...${NC}"
  [ -n "$BOOTSTRAP_PID" ] && kill $BOOTSTRAP_PID 2>/dev/null || true
  wait $BOOTSTRAP_PID 2>/dev/null || true
  exit ${1:-0}
}

# Setup cleanup on script exit or interrupt
trap 'cleanup $?' EXIT
trap 'cleanup 1' INT

echo -e "${YELLOW}Step 1: Starting bootstrap node on port $BOOTSTRAP_PORT${NC}"
cargo run --example simple_node -- --port $BOOTSTRAP_PORT bootstrap > bootstrap.log 2>&1 &
BOOTSTRAP_PID=$!

echo -e "${YELLOW}Waiting for bootstrap node to initialize (5 seconds)...${NC}"
sleep 5

# Check if bootstrap node is running
if ! ps -p $BOOTSTRAP_PID > /dev/null; then
    echo -e "${RED}ERROR: Bootstrap node failed to start. Check bootstrap.log for details.${NC}"
    cat bootstrap.log
    exit 1
fi

echo -e "${GREEN}Bootstrap node started successfully.${NC}"

# Test PING operation
echo -e "${YELLOW}Step 2: Testing node connection via 'join' command...${NC}"
JOIN_OUTPUT=$(cargo run --example simple_node -- --port $CLIENT_PORT join --bootstrap 127.0.0.1:$BOOTSTRAP_PORT 2>&1)

if echo "$JOIN_OUTPUT" | grep -q "Successfully joined the Kademlia network"; then
    echo -e "${GREEN}JOIN TEST PASSED: Successfully connected to bootstrap node.${NC}"
    echo -e "${YELLOW}Join operation output excerpt:${NC}"
    echo "$JOIN_OUTPUT" | grep -E "Node started|Connecting|Success"
else
    echo -e "${RED}JOIN TEST FAILED: Could not connect to bootstrap node.${NC}"
    echo -e "${RED}Join operation output:${NC}"
    echo "$JOIN_OUTPUT"
    exit 1
fi

echo -e "${YELLOW}Test completed successfully - basic node-to-node communication works!${NC}"
echo -e "${YELLOW}For a complete DHT test, the implementation needs to be fixed to handle key distribution properly.${NC}"

exit 0