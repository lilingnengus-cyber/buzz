#!/usr/bin/env python3
"""Deliver Xiake inquiries to CRM using the internal authenticated service API.

Run on the business server. No CRM credentials are exposed to the website.
The website submission UUID is the permanent idempotency key; acknowledgement
happens only after CRM confirms creation. A timer retries unacknowledged rows.
"""
import json
import os
import sys
import urllib.request
import uuid


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def request_json(url, headers, payload=None):
    data = None if payload is None else json.dumps(payload).encode()
    request = urllib.request.Request(url, data=data, headers={
        **headers, 'Content-Type': 'application/json', 'User-Agent': 'Xiake-CRM-Sync/1.0',
    })
    with urllib.request.build_opener(NoRedirect).open(request, timeout=20) as response:
        return json.load(response)


def payload_for(lead, owner):
    submission = str(uuid.UUID(lead['id']))
    summary = (lead['challenge'] + '\n\n官网需求编号：' + submission
               + '\n提交入口：' + lead['source'])
    if lead.get('stage'):
        summary += '\n企业阶段：' + lead['stage']
    if lead.get('brief'):
        summary += '\n完整需求简报及后续补充：https://xiakeyuzhou.com/admin'
    return {
        'title': lead['company'] + ' · 官网咨询',
        'companyName': lead['company'], 'contactName': lead['name'],
        'contactDetails': lead['contact'], 'source': '官网',
        'summary': summary, 'ownerUserId': owner,
    }


def synchronize(site, core, token, service_credential, owner, request=request_json):
    owner = str(uuid.UUID(owner))
    site_headers = {'Authorization': 'Bearer ' + token}
    core_headers = {
        'x-business-service-credential': service_credential,
        'x-service-audience': 'business-core', 'x-enterprise-user-id': owner,
    }
    records = request(site + '/api/integrations/crm/pending', site_headers)['items']
    succeeded = failed = 0
    for lead in records:
        try:
            submission = str(uuid.UUID(lead['id']))
            result = request(core + '/v1/agent-crm/leads', {
                **core_headers, 'Idempotency-Key': 'xiake-web:' + submission,
            }, payload_for(lead, owner))
            crm_id = str(uuid.UUID(result['id']))
            request(site + '/api/integrations/crm/' + submission + '/ack',
                    site_headers, {'crmLeadId': crm_id})
            succeeded += 1
        except Exception as error:
            # Never log payloads, contacts, tokens or response bodies.
            print('CRM delivery failed: ' + type(error).__name__, file=sys.stderr)
            failed += 1
    return succeeded, failed


if __name__ == '__main__':
    try:
        saved, failed = synchronize(
            'https://xiakeyuzhou.com', os.environ['CRM_CORE_URL'],
            os.environ['CRM_SYNC_TOKEN'], os.environ['BUSINESS_CORE_SERVICE_CREDENTIAL'],
            os.environ['CRM_OWNER_ID'])
        if saved or failed:
            print(f'CRM delivery: saved={saved} failed={failed}')
        sys.exit(1 if failed else 0)
    except Exception as error:
        print('CRM delivery unavailable: ' + type(error).__name__, file=sys.stderr)
        sys.exit(1)
