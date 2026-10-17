import concurrent.futures
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import time
import urllib.request

RUN = Path(__file__).parent
ROOT = Path("/home/johnreed/Desktop/work/subtensor")
PYTHON = ROOT / "sdk/python/.venv/bin/python"
chains = json.loads((RUN / "chains.json").read_text())
launchers = []
children = []
results = {}
suites = {
    "sponsorship": ("test_mldsa_sponsorship_live.py", 4),
    "nested": ("test_mldsa_nested_authority.py", 0),
    "validator-proxy": ("test_mldsa_validator_proxy.py", 2),
    "mixed": ("test_mixed_multisig.py", 6),
    "shield": ("test_wallet_shield.py", 6),
    "release": ("test_mldsa_release.py", 0),
    "authority": ("test_mldsa_localnet.py", 3),
    "operations": ("test_mldsa_operation_families.py", 4),
    "validators": ("test_mldsa_validator_paths.py", 2),
    "lifecycle": ("test_wallet_types.py", 6),
    "parity": ("test_wallet_parity.py", 6),
    "extended": ("test_mldsa_extended.py", 0),
    "proxy-boundaries": ("test_mldsa_proxy_boundaries.py", 2),
}

def rpc(port, method):
    data = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": []}).encode()
    request = urllib.request.Request(f"http://127.0.0.1:{port}", data, {"Content-Type": "application/json"})
    return json.load(urllib.request.urlopen(request, timeout=5))["result"]

def run(chain):
    name = chain["name"]
    directory = Path(chain["directory"])
    env = {**os.environ, "PYTHONPATH": str(RUN / "native") + ":" + str(ROOT / "sdk/python"),
           "E2E_WALLET_ENDPOINT": f"ws://127.0.0.1:{chain['rpc_port']}",
           "E2E_WALLET_ALICE_LOCK": str(directory / "alice.lock"),
           "E2E_EVIDENCE_DIR": str(directory / "receipts"),
           "E2E_EVM_ASSOCIATION_EXPECT_RATE_LIMIT": "1",
           "E2E_WALLET_PEERS": ",".join(f"ws://127.0.0.1:{p}" for p in range(chain["rpc_port"],chain["rpc_port"]+3))}
    deadline = time.monotonic() + 240
    while time.monotonic() < deadline:
        try:
            if int(rpc(chain["rpc_port"], "chain_getHeader")["number"], 16) > 2:
                break
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(2)
    else:
        return {"error": "chain startup timeout"}
    if name == "rpc":
        with (directory / "runner.log").open("w") as output:
            process = subprocess.Popen([str(PYTHON), str(directory / "run.py")], cwd=ROOT / "sdk/python", env=env, stdout=output, stderr=subprocess.STDOUT)
            children.append(process)
            return {"rpc_exit": process.wait(timeout=7200)}
    with (directory / "activation.log").open("w") as output:
        process = subprocess.Popen([str(PYTHON), str(RUN / "activate.py"), name, str(chain["rpc_port"])],
                                   cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT)
        children.append(process)
        code = process.wait(timeout=300)
    if code:
        return {"activation_exit": code}
    if name == "production":
        return {"production_gate_exit": 0}
    filename, workers = suites[name]
    # Test files may be completed by independent review agents during build/startup.
    deadline = time.monotonic() + 900
    while not (ROOT / "sdk/python/tests/e2e" / filename).exists():
        if time.monotonic() > deadline:
            return {"error": "missing new suite"}
        time.sleep(2)
    args = [str(PYTHON), "-m", "pytest", f"tests/e2e/{filename}", "-vv", "--tb=short", "-x",
            f"--junitxml={directory / 'results.xml'}"]
    if workers:
        args.extend(["-n", str(workers), "--dist=load"])
    start = time.time()
    with (directory / "pytest.log").open("w") as output:
        process = subprocess.Popen(args, cwd=ROOT / "sdk/python", env=env,
                                   stdout=output, stderr=subprocess.STDOUT)
        children.append(process)
        code = process.wait(timeout=10800)
    return {"pytest_exit": code, "seconds": round(time.time() - start, 2)}

def shutdown():
    for child in children:
        if child.poll() is None:
            child.terminate()
    for launcher in launchers:
        if launcher.poll() is None:
            launcher.terminate()
    for launcher in launchers:
        try:
            launcher.wait(timeout=45)
        except subprocess.TimeoutExpired:
            launcher.kill()
    ports = []
    for chain in chains:
        for base in (chain["rpc_port"], chain["p2p_port"]):
            for port in range(base, base + 3):
                with socket.socket() as sock:
                    ports.append({"port": port, "closed": sock.connect_ex(("127.0.0.1", port)) != 0})
    (RUN / "shutdown.json").write_text(json.dumps({"ports": ports, "all_closed": all(p["closed"] for p in ports)}, indent=2))

def interrupted(signum, frame):
    raise KeyboardInterrupt

signal.signal(signal.SIGTERM, interrupted)
signal.signal(signal.SIGINT, interrupted)
try:
    for chain in chains:
        directory = Path(chain["directory"])
        for port in (chain["rpc_port"], chain["p2p_port"]):
            with socket.socket() as sock:
                assert sock.connect_ex(("127.0.0.1", port)) != 0, f"port busy: {port}"
        with (directory / "localnet.log").open("w") as output:
            launcher = subprocess.Popen(["bash", str(directory / "localnet.sh"), "False"],
                                        env={**os.environ, "BUILD_BINARY": "0"}, cwd=ROOT,
                                        stdout=output, stderr=subprocess.STDOUT)
        launchers.append(launcher)
    (RUN / "launchers.json").write_text(json.dumps([dict(name=c["name"], pid=p.pid) for c,p in zip(chains,launchers)], indent=2))
    with concurrent.futures.ThreadPoolExecutor(max_workers=len(chains)) as pool:
        futures = {pool.submit(run, chain): chain["name"] for chain in chains}
        for future in concurrent.futures.as_completed(futures):
            name = futures[future]
            try:
                results[name] = future.result()
            except Exception as error:
                results[name] = {"error": repr(error)}
            (RUN / "live-results.json").write_text(json.dumps(results, indent=2))
            print(name, results[name], flush=True)
    print("ALL_SUITES_FINISHED", flush=True)
    while not (RUN / "stop").exists():
        time.sleep(2)
finally:
    shutdown()
