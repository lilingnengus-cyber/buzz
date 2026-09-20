#!/usr/bin/env python3
"""Exercise a paired Linux candidate in an isolated CI Docker network."""
import json
import os
import subprocess
import time
import urllib.error
import urllib.request
import uuid

if os.environ.get("GITHUB_ACTIONS") != "true":
    raise SystemExit("Run only in the isolated GitHub Actions job")

SOURCE = "c186ddf01119acfb545c56dee7b77304874e282b"
NETWORK = "business-hold-runtime"
CREDENTIAL = "isolated-runtime-credential-" + uuid.uuid4().hex
DB = "postgres://rehearsal:rehearsal@hold-postgres:5432/rehearsal"


def docker(*args):
    return subprocess.check_output(["docker", *args], text=True).strip()


def request(port, path, payload=None, authenticated=True):
    headers = {"Content-Type": "application/json"}
    if authenticated:
        headers.update({
            "x-business-service-credential": CREDENTIAL,
            "x-service-audience": "business-core",
            "x-enterprise-user-id": "00000000-0000-4000-8000-000000000001",
            "x-trace-id": str(uuid.uuid4()),
            "idempotency-key": str(uuid.uuid4()),
        })
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}{path}",
        data=None if payload is None else json.dumps(payload).encode(), headers=headers,
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as response:
            return response.status
    except urllib.error.HTTPError as error:
        return error.code


containers = []
try:
    docker("network", "create", NETWORK)
    docker("run", "-d", "--name", "hold-postgres", "--network", NETWORK,
           "-e", "POSTGRES_USER=rehearsal", "-e", "POSTGRES_PASSWORD=rehearsal",
           "-e", "POSTGRES_DB=rehearsal", "postgres:17")
    containers.append("hold-postgres")
    for _ in range(60):
        if subprocess.run(["docker", "exec", "hold-postgres", "pg_isready", "-U", "rehearsal"],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0:
            break
        time.sleep(1)
    else:
        raise AssertionError("Postgres did not become ready")
    docker("run", "--rm", "--network", NETWORK, "-e", f"DATABASE_URL={DB}",
           f"shiyue-business-candidate-gateway:{SOURCE}", "--migrate-only")
    version = docker("exec", "hold-postgres", "psql", "-U", "rehearsal", "-d", "rehearsal",
                     "-Atc", "SELECT max(version) FROM _sqlx_migrations WHERE success")
    assert version == "59", version
    ports = {}
    for mode, image in [("normal", "business-core"), ("paused", "writes-paused")]:
        name = "hold-" + mode
        docker("run", "-d", "--name", name, "--network", NETWORK,
               "-p", "127.0.0.1::3110", "-e", f"BUSINESS_CORE_DATABASE_URL={DB}",
               "-e", "BUSINESS_CORE_BIND_ADDR=0.0.0.0:3110",
               "-e", f"BUSINESS_CORE_SERVICE_CREDENTIAL={CREDENTIAL}",
               "-e", "BUSINESS_WEB_ORIGIN=http://localhost",
               f"shiyue-business-candidate-{image}:{SOURCE}")
        containers.append(name)
        ports[mode] = int(docker("port", name, "3110/tcp").rsplit(":", 1)[1])
        for _ in range(60):
            try:
                if request(ports[mode], "/health", authenticated=False) == 200:
                    break
            except OSError:
                pass
            time.sleep(1)
        else:
            raise AssertionError(f"{mode} Core did not become ready")
    evidence = {"source": SOURCE, "migration": version, "routes": []}
    for path in [
        "/v1/agent-order-hold-previews/sales_order_hold_intent",
        "/v1/agent-order-hold-previews/sales_order_release_hold_intent",
        "/v1/agent-order-hold-intents/sales_order_hold_intent",
        "/v1/agent-order-hold-intents/sales_order_release_hold_intent",
        "/v1/agent-master-intents/core_master_status_intent",
        "/v1/agent-master-intents/product_master_status_intent",
    ]:
        normal = request(ports["normal"], path, {})
        paused = request(ports["paused"], path, {})
        assert normal in (400, 422), (path, normal)
        assert paused == 503, (path, paused)
        assert request(ports["paused"], path, {}, authenticated=False) == 401
        evidence["routes"].append({"path": path, "normal": normal, "paused": paused})
    # Existing read routes still reach their permission checks in both images.
    normal = request(ports["normal"], "/v1/sales-orders")
    paused = request(ports["paused"], "/v1/sales-orders")
    assert normal == paused and normal in (401, 403), (normal, paused)
    evidence["existingRead"] = {"normal": normal, "paused": paused}
    with open("runtime-evidence.json", "w") as output:
        json.dump(evidence, output, indent=2)
    print(json.dumps(evidence))
finally:
    for name in reversed(containers):
        subprocess.run(["docker", "logs", name], stdout=open(name + ".log", "w"),
                       stderr=subprocess.STDOUT, check=False)
        subprocess.run(["docker", "rm", "-f", name], check=False)
    subprocess.run(["docker", "network", "rm", NETWORK], check=False)
