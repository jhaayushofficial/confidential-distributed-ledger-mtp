# Cloud Setup Guide: 50+ Validator PBFT vs DAG-BFT Benchmark

## What You Get from GitHub Student Developer Pack

| Provider        | Credit   | Best For               | Claim URL |
|----------------|----------|------------------------|-----------|
| **DigitalOcean** | **$200** for 1 year | FISCO 50-node cluster | https://education.github.com/pack → DigitalOcean |
| **Microsoft Azure** | $100 | Alternative VM option  | https://azure.microsoft.com/en-us/free/students/ |
| **Namecheap**   | Free domain | Not needed here       | — |

**Recommended**: Use **DigitalOcean** ($200 credit) — simplest setup, cheapest VMs.

---

## Architecture Plan

We need 50+ validators for both FISCO (PBFT) and Sui (DAG-BFT).

### Option A: Single Large VM (Simplest — Recommended)

| What | Spec | Cost |
|------|------|------|
| 1× DigitalOcean Droplet | 32 GB RAM, 8 vCPU (s-8vcpu-32gb) | $0.238/hr ≈ $192/month |
| Run 50-node FISCO | ~15 GB RAM | On same VM |
| Run 50-validator Sui localnet | ~15 GB RAM | After FISCO (sequential) |

**Total cost for ~2 hours of benchmarking: ~$0.50**

### Option B: Distributed Multi-VM (More Realistic, More Complex)

| What | Spec | Count | Cost |
|------|------|-------|------|
| DigitalOcean Droplets | 8 GB RAM, 4 vCPU | 5 VMs | $0.048/hr each |
| 10 FISCO nodes per VM | ~3 GB RAM/VM | 50 total | — |

**Total cost for ~3 hours: ~$0.75**

**We'll use Option A** — simpler, same TPS results (network latency on localhost is negligible for PBFT message complexity comparison).

---

## Step-by-Step Setup

### Phase 1: Claim DigitalOcean Credits

1. Go to https://education.github.com/pack
2. Sign in with your GitHub account (must have Student Developer Pack verified)
3. Find **DigitalOcean** → Click "Get access"
4. Create a DigitalOcean account (or link existing)
5. Apply the $200 credit promo code from the Student Pack
6. Verify at https://cloud.digitalocean.com/account/billing → Credits should show $200

### Phase 2: Create the VM (Droplet)

#### Via Web UI:
1. Go to https://cloud.digitalocean.com → **Create** → **Droplets**
2. Choose:
   - **Region**: Closest to you (e.g., `sgp1` Singapore or `blr1` Bangalore)
   - **Image**: **Ubuntu 22.04 LTS**
   - **Size**: **General Purpose** → **s-8vcpu-32gb** ($192/month, hourly billing)
     - If $200 budget is tight, use **s-4vcpu-16gb** ($96/month) — can still run 50 nodes with memory patches
   - **Authentication**: **SSH Key** (recommended) or Password
   - **Hostname**: `benchmark-vm`
3. Click **Create Droplet**
4. Note the **public IP** (e.g., `164.92.xxx.xxx`)

#### Via CLI (faster):
```bash
# Install doctl (DigitalOcean CLI)
# On Windows: winget install DigitalOcean.Doctl
# On WSL: sudo snap install doctl

# Authenticate
doctl auth init    # Paste your API token from DO dashboard

# Create SSH key (if you don't have one)
ssh-keygen -t ed25519 -f ~/.ssh/do_benchmark -N ""

# Upload SSH key
doctl compute ssh-key create benchmark-key --public-key "$(cat ~/.ssh/do_benchmark.pub)"

# Get the key ID
KEY_ID=$(doctl compute ssh-key list --format ID --no-header | head -1)

# Create 32GB droplet
doctl compute droplet create benchmark-vm \
  --region sgp1 \
  --image ubuntu-22-04-x64 \
  --size s-8vcpu-32gb \
  --ssh-keys $KEY_ID \
  --wait

# Get the IP
doctl compute droplet list --format Name,PublicIPv4
```

### Phase 3: SSH into the VM

```bash
# From WSL on your laptop
ssh -i ~/.ssh/do_benchmark root@<DROPLET_IP>

# Or if you set a password:
ssh root@<DROPLET_IP>
```

### Phase 4: Install Dependencies on the VM

```bash
# Update system
apt update && apt upgrade -y

# Install build tools
apt install -y build-essential curl git pkg-config libssl-dev \
  cmake clang protobuf-compiler python3 netcat-openbsd jq unzip

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# Verify
rustc --version   # should be 1.7x+
cargo --version
```

### Phase 5: Upload Your Code to the VM

#### Option A: Git (if you have a private repo)
```bash
# On the VM
git clone https://github.com/YOUR_USER/YOUR_REPO.git ~/MTP
```

#### Option B: SCP from your laptop (recommended — no repo needed)
```bash
# From WSL on your laptop — upload the entire project
cd /mnt/c/MTP

# Create a tarball excluding build artifacts
tar czf /tmp/mtp_project.tar.gz \
  --exclude='*/target' \
  --exclude='*/build' \
  --exclude='FISCO-BCOS' \
  --exclude='console/lib' \
  --exclude='console/apps' \
  --exclude='*.o' \
  --exclude='*.so' \
  "Confidential Distributed Ledgers/" \
  "console/contracts/" \
  "console/account/" \
  run_multi_node_bench.sh

# Upload to VM
scp -i ~/.ssh/do_benchmark /tmp/mtp_project.tar.gz root@<DROPLET_IP>:/root/

# On the VM — extract
cd /root
tar xzf mtp_project.tar.gz
mv "Confidential Distributed Ledgers" cdl
```

Also upload the FISCO binary and build_chain.sh:
```bash
# From WSL on your laptop
scp -i ~/.ssh/do_benchmark ~/fisco/fisco-bcos root@<DROPLET_IP>:/root/
scp -i ~/.ssh/do_benchmark ~/fisco/build_chain.sh root@<DROPLET_IP>:/root/

# Also upload the compiled Solidity contracts
scp -i ~/.ssh/do_benchmark -r /mnt/c/MTP/console/contracts/.compiled root@<DROPLET_IP>:/root/compiled_contracts/
```

---

## FISCO-BCOS 50-Node Benchmark (PBFT)

### Step 1: Set Up FISCO on the VM

```bash
# On the VM
mkdir -p ~/fisco
cp ~/fisco-bcos ~/fisco/fisco-bcos
cp ~/build_chain.sh ~/fisco/build_chain.sh
chmod +x ~/fisco/fisco-bcos ~/fisco/build_chain.sh

# Verify binary works
~/fisco/fisco-bcos -v
# Should print: FISCO-BCOS Version : 3.6.0
```

### Step 2: Generate 50-Node Network

```bash
cd ~/fisco

# Generate 50 nodes on localhost
bash build_chain.sh -l "127.0.0.1:50" -e ~/fisco/fisco-bcos -o ~/fisco/nodes_50

# Patch configs for memory optimization
for ini in ~/fisco/nodes_50/127.0.0.1/node*/config.ini; do
  sed -i 's/key_page_size=.*/key_page_size=4096/' "$ini"
  sed -i 's/limit=.*/limit=500/' "$ini"
  sed -i 's/notify_worker_num=.*/notify_worker_num=1/' "$ini"
  sed -i 's/thread_count=.*/thread_count=1/' "$ini"
  sed -i 's/level=info/level=warning/' "$ini"
  sed -i 's/max_log_file_size=.*/max_log_file_size=128/' "$ini"
  sed -i 's/enable_dag=true/enable_dag=false/' "$ini"
done

# Patch genesis compatibility version
for genesis in ~/fisco/nodes_50/127.0.0.1/node*/config.genesis; do
  sed -i 's/compatibility_version=.*/compatibility_version=3.6.0/' "$genesis"
done

echo "50 nodes configured."
```

### Step 3: Start All Nodes

```bash
ulimit -n 65536

count=0
for start_sh in ~/fisco/nodes_50/127.0.0.1/node*/start.sh; do
  bash "$start_sh" > /dev/null 2>&1 &
  count=$((count + 1))
  # Batch pause every 10 nodes for memory stability
  if [ $((count % 10)) -eq 0 ]; then
    echo "Started $count/50 nodes, pausing 3s..."
    sleep 3
  fi
done
echo "All $count nodes started."

# Wait for PBFT consensus (50 nodes need ~170s to stabilize)
echo "Waiting 180s for PBFT consensus to stabilize..."
sleep 180

# Check nodes are running
echo "Running FISCO nodes: $(pgrep -c fisco-bcos || echo 0)"

# Quick RPC check
curl -k -s -X POST https://127.0.0.1:20200 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"getBlockNumber","params":["group0"],"id":1}' | jq .
```

### Step 4: Stage TLS Certs and Build Rust Binaries

```bash
# Stage SDK certs
mkdir -p ~/fisco/active_certs
cp ~/fisco/nodes_50/127.0.0.1/sdk/ca.crt ~/fisco/active_certs/
cp ~/fisco/nodes_50/127.0.0.1/sdk/sdk.crt ~/fisco/active_certs/ 2>/dev/null || \
  cp ~/fisco/nodes_50/127.0.0.1/sdk/node.crt ~/fisco/active_certs/sdk.crt
cp ~/fisco/nodes_50/127.0.0.1/sdk/sdk.key ~/fisco/active_certs/ 2>/dev/null || \
  cp ~/fisco/nodes_50/127.0.0.1/sdk/node.key ~/fisco/active_certs/sdk.key

# Build the Rust benchmark binaries
cd ~/cdl/fisco_ledger
cargo build --release --bin tps_bench --bin deploy
```

### Step 5: Configure and Deploy Contracts

```bash
# Create/update ledger_config.json
cat > ~/cdl/fisco_ledger/ledger_config.json << 'EOF'
{
  "rpc_url": "https://127.0.0.1:20200",
  "group_id": "group0",
  "chain_id": "chain0",
  "loan_ledger_address": "",
  "repayment_ledger_address": "",
  "account_address": "0x0000000000000000000000000000000000000000",
  "ca_cert": "/root/fisco/active_certs/ca.crt",
  "sdk_cert": "/root/fisco/active_certs/sdk.crt",
  "sdk_key": "/root/fisco/active_certs/sdk.key",
  "private_key_pem": ""
}
EOF

# NOTE: The deploy binary needs a secp256k1 private key.
# Option 1: Generate one on the VM
# Option 2: Copy your existing .pem from laptop

# Copy the PEM key from laptop:
# (from your WSL)
# scp -i ~/.ssh/do_benchmark /mnt/c/MTP/console/account/ecdsa/0x8f29d04cf7ea2df99b99ca8f5d823f939b94eb98.pem root@<DROPLET_IP>:/root/account.pem

# Update config with the PEM path
# Then update ledger_config.json
python3 -c "
import json
with open('/root/cdl/fisco_ledger/ledger_config.json') as f:
    cfg = json.load(f)
cfg['private_key_pem'] = '/root/account.pem'
with open('/root/cdl/fisco_ledger/ledger_config.json', 'w') as f:
    json.dump(cfg, f, indent=2)
print('Config updated')
"

# Make sure compiled contract bytecode is accessible
# Copy from laptop: the deploy binary reads from specific paths
# Check what paths deploy.rs expects and create symlinks
mkdir -p /mnt/c/MTP/console/contracts/.compiled/ 2>/dev/null || true
# If running natively (not WSL), you may need to adjust paths in deploy.rs
# OR just copy compiled contracts:
mkdir -p ~/contracts
cp ~/compiled_contracts/*.bin ~/contracts/
# You may need to update deploy.rs paths — see troubleshooting section below

# Deploy contracts
cd ~/cdl/fisco_ledger
./target/release/deploy
```

### Step 6: Run FISCO 50-Node Benchmark

```bash
cd ~/cdl/fisco_ledger

# Test with n=10 lenders first (quick sanity check)
./target/release/tps_bench --duration 15 --only-n 10 --concurrent 64

# Full benchmark: n=10, 50, 100
./target/release/tps_bench --duration 30 --concurrent 200

# Save results
./target/release/tps_bench --duration 30 --concurrent 200 | tee ~/fisco_50node_results.txt
```

### Step 7: Stop FISCO Nodes

```bash
pkill -f fisco-bcos
sleep 2
echo "Remaining: $(pgrep -c fisco-bcos || echo 0)"
```

---

## Sui 50-Validator Benchmark (DAG-BFT / Mysticeti)

### Step 1: Install Sui CLI on the VM

```bash
# Download pre-built Sui binary (fastest)
# Check latest version at https://github.com/MystenLabs/sui/releases
cd /tmp

# For Ubuntu x86_64:
curl -L -o sui.tgz https://github.com/MystenLabs/sui/releases/download/testnet-v1.44.2/sui-testnet-v1.44.2-ubuntu-x86_64.tgz
# NOTE: Replace with latest release URL. Check:
# https://github.com/MystenLabs/sui/releases

tar xzf sui.tgz
mv sui /usr/local/bin/sui
chmod +x /usr/local/bin/sui
sui --version

# If pre-built doesn't work, build from source (takes ~20 min):
# cargo install --locked --git https://github.com/MystenLabs/sui.git --branch testnet sui
```

### Step 2: Generate 50-Validator Genesis

```bash
# Create Sui working directory
mkdir -p ~/sui-work

# Generate 50-validator localnet genesis
# This creates validator configs, genesis blob etc.
sui genesis --working-dir ~/sui-work --committee-size 50

# Check what was generated
ls ~/sui-work/
# Should see: genesis.blob, validator-configs/
```

### Step 3: Start 50-Validator Localnet

```bash
# Start localnet with 50 validators using the generated config
sui start --network.config ~/sui-work/network.yaml &> ~/sui_50val.log &

# Wait for validators to start (50 validators need ~60s)
echo "Waiting 90s for Sui validators to initialize..."
sleep 90

# Check if RPC is responding
curl -s http://127.0.0.1:9000 -X POST \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"sui_getLatestCheckpointSequenceNumber","params":[]}' | jq .
```

**Alternative (simpler) — if `sui genesis` + `sui start` doesn't support 50 directly:**
```bash
# Use the built-in localnet command
# Recent Sui versions support --committee-size flag:
sui start --local --committee-size 50 &> ~/sui_50val.log &
sleep 90

# Check
curl -s http://127.0.0.1:9000 -X POST \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"sui_getLatestCheckpointSequenceNumber","params":[]}' | jq .
```

### Step 4: Publish the Move Contract

```bash
# Set up Sui client config
sui client new-env --alias local --rpc http://127.0.0.1:9000
sui client switch --env local

# Get Sui address
SUI_ADDR=$(sui client active-address)
echo "Address: $SUI_ADDR"

# Fund from localnet faucet
curl -X POST http://127.0.0.1:9123/gas \
  -H 'Content-Type: application/json' \
  -d "{\"FixedAmountRequest\":{\"recipient\":\"$SUI_ADDR\"}}"
sleep 3

# Publish the Move contract
cd ~/cdl/sui_ledger/move/loan_ledger

# Clean any stale publish state
rm -f Pub.*.toml

# Publish
sui client publish --gas-budget 100000000 --json | tee /tmp/publish_output.json

# Extract package ID and ledger object ID
PACKAGE_ID=$(python3 -c "
import json, sys
data = json.load(open('/tmp/publish_output.json'))
for change in data.get('objectChanges', []):
    if change.get('type') == 'published':
        print(change['packageId'])
        break
")

LEDGER_ID=$(python3 -c "
import json
data = json.load(open('/tmp/publish_output.json'))
for change in data.get('objectChanges', []):
    if change.get('type') == 'created' and 'LoanLedger' in change.get('objectType', ''):
        print(change['objectId'])
        break
")

echo "Package ID: $PACKAGE_ID"
echo "Ledger ID:  $LEDGER_ID"

# Get initial shared version
INITIAL_VERSION=$(python3 -c "
import json
data = json.load(open('/tmp/publish_output.json'))
for change in data.get('objectChanges', []):
    if change.get('type') == 'created' and 'LoanLedger' in change.get('objectType', ''):
        owner = change.get('owner', {})
        if isinstance(owner, dict) and 'Shared' in owner:
            print(owner['Shared']['initial_shared_version'])
        break
")
echo "Initial shared version: $INITIAL_VERSION"
```

### Step 5: Update Sui Config and Build Benchmark

```bash
# Update sui_ledger_config.json
cat > ~/cdl/sui_ledger/sui_ledger_config.json << EOF
{
  "rpc_url": "http://127.0.0.1:9000",
  "faucet_url": "http://127.0.0.1:9123/gas",
  "package_id": "$PACKAGE_ID",
  "ledger_object_id": "$LEDGER_ID",
  "ledger_initial_shared_version": $INITIAL_VERSION
}
EOF

cat ~/cdl/sui_ledger/sui_ledger_config.json

# Build the Sui benchmark binary
cd ~/cdl/sui_ledger
cargo build --release --bin sui_tps_bench
```

### Step 6: Run Sui 50-Validator Benchmark

```bash
cd ~/cdl/sui_ledger

# Quick sanity check
./target/release/sui_tps_bench --duration 15 --concurrent 32

# Full benchmark
./target/release/sui_tps_bench --duration 30 --concurrent 64 | tee ~/sui_50val_results.txt

# Also run original O(n) mode for comparison
./target/release/sui_tps_bench --duration 30 --concurrent 64 --original | tee ~/sui_50val_original_results.txt
```

### Step 7: Stop Sui

```bash
pkill -f 'sui start' || pkill -f sui-node
```

---

## Bonus: 100-Validator Experiments

### FISCO 100-Node (needs 64 GB RAM)

```bash
# Create a 64 GB droplet ($384/month, ~$0.57/hr)
doctl compute droplet create benchmark-100 \
  --region sgp1 \
  --image ubuntu-22-04-x64 \
  --size m-8vcpu-64gb \
  --ssh-keys $KEY_ID \
  --wait

# Then follow same FISCO steps above but with:
bash build_chain.sh -l "127.0.0.1:100" -e ~/fisco/fisco-bcos -o ~/fisco/nodes_100
# Wait 200s for PBFT consensus
# Expect: very low TPS (~5-10) due to O(n²) PBFT message complexity
```

### Sui 100-Validator
For Sui with 100+ validators, you have two options:
1. **64 GB VM** with `--committee-size 100` (expensive but controlled)
2. **Use Sui devnet** (free, already has 100+ real validators globally distributed)
   - Your contract is already deployed on devnet:
     - Package: `0x37ee338e8c345b3fa5da58ada463c09d4a5c17ebf80add9749ffead3a23b2de0`
     - Ledger: `0x939efff1e6431566e92721fce324c13c21ff217157c43203e13493d67e882bf7`
   - Just run: `./sui_tps_bench --duration 30 --concurrent 8` (with devnet config)

---

## Quick Reference: Cost Estimates

| Experiment | VM Size | Cost/hr | Est. Time | Total Cost |
|-----------|---------|---------|-----------|------------|
| FISCO 50-node | 32 GB | $0.238 | 1 hr | ~$0.24 |
| Sui 50-val | 32 GB | $0.238 | 1 hr | ~$0.24 |
| FISCO 100-node | 64 GB | $0.571 | 1 hr | ~$0.57 |
| Sui 100-val | 64 GB | $0.571 | 1 hr | ~$0.57 |
| **Total (all 4)** | — | — | **4 hrs** | **~$1.62** |

You have $200 credit — this costs less than $2 total. Even if you repeat experiments 10×, you'll spend < $20.

---

## Troubleshooting

### Problem: `deploy` binary can't find .bin contract files
The deploy binary reads from hardcoded `/mnt/c/MTP/console/contracts/.compiled/` paths.
**Fix**: Either create that directory structure on the VM, or create a symlink:
```bash
mkdir -p /mnt/c/MTP/console/contracts/
ln -s ~/compiled_contracts /mnt/c/MTP/console/contracts/.compiled
```
Or edit `deploy.rs` to use a relative/configurable path and rebuild.

### Problem: FISCO nodes crash (OOM)
```bash
# Check memory usage
free -h
# If using 16 GB VM, reduce to 30 nodes, or upgrade to 32 GB
```

### Problem: Sui 50 validators too heavy
```bash
# Check memory
free -h
# If tight, reduce to 30 validators:
sui genesis --working-dir ~/sui-work --committee-size 30
```

### Problem: Can't SSH into the VM
```bash
# Check droplet status
doctl compute droplet list
# Ensure SSH key was correctly added
# Try with password auth: doctl compute droplet create ... --password
```

### Problem: Sui binary download URL is wrong
```bash
# Go to https://github.com/MystenLabs/sui/releases
# Find the latest testnet release for ubuntu-x86_64
# Download that instead
```

---

## Complete Benchmark Execution Checklist

```
□ 1. Claim DigitalOcean $200 credit
□ 2. Create 32 GB droplet (Ubuntu 22.04)
□ 3. SSH in, install dependencies (Rust, build tools)
□ 4. Upload code + FISCO binary + contract bytecode
□ 5. Generate FISCO 50-node network
□ 6. Patch configs (memory, genesis version)
□ 7. Start nodes, wait for consensus
□ 8. Deploy contracts, run FISCO benchmark → save results
□ 9. Stop FISCO nodes
□ 10. Install Sui CLI
□ 11. Generate 50-validator genesis, start localnet
□ 12. Publish Move contract
□ 13. Run Sui benchmark → save results
□ 14. Stop Sui
□ 15. Download results: scp root@<IP>:~/fisco_50node_results.txt .
□ 16. DESTROY the droplet (stop billing!)
     doctl compute droplet delete benchmark-vm --force
```

**CRITICAL: Delete the droplet when done to stop billing!**
```bash
doctl compute droplet delete benchmark-vm --force
# Or via web UI: Droplets → benchmark-vm → Destroy
```

---

## Expected Results (for your paper)

| Validators | FISCO PBFT (TPS) | Sui DAG-BFT (TPS) | PBFT Degradation |
|-----------|------------------|-------------------|-----------------|
| 4         | ~125             | ~100              | baseline        |
| 10        | ~121             | ~100              | -3%             |
| 50        | **~14** (predicted) | **~80-90** (predicted) | **-89%**   |
| 100       | **~5-10**        | **~70-75**        | **-95%**        |

The key finding: **PBFT TPS degrades O(n²) while DAG-BFT remains near-constant.**
