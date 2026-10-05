#!/usr/bin/env python3
"""Resolve current Core container without pinning a stale Docker address."""
import json
import subprocess
import sys
from pathlib import Path
from sync import synchronize

try:
    config = json.loads(Path('/etc/xiake-crm-sync.json').read_text())
    container = json.loads(subprocess.check_output([
        'docker', 'inspect', 'shiyue-business-platform-business-core-1',
    ]))[0]
    env = dict(item.split('=', 1) for item in container['Config']['Env'])
    address = container['NetworkSettings']['Networks']['shiyue-business-platform_default']['IPAddress']
    port = env.get('BUSINESS_CORE_BIND_ADDR', '0.0.0.0:8080').rsplit(':', 1)[1]
    saved, failed = synchronize('https://xiakeyuzhou.com', f'http://{address}:{port}',
                                config['token'], env['BUSINESS_CORE_SERVICE_CREDENTIAL'], config['owner'])
    if saved or failed:
        print(f'CRM delivery: saved={saved} failed={failed}')
    sys.exit(1 if failed else 0)
except Exception as error:
    print('CRM delivery unavailable: ' + type(error).__name__, file=sys.stderr)
    sys.exit(1)
