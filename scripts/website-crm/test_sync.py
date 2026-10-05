import unittest
from sync import synchronize, payload_for

ID = 'c4935e8b-beb4-4d66-b3fd-104406473c0f'
OWNER = '68788b86-19ad-4a90-aa9f-4ea0fb74bd3e'
LEAD = dict(id=ID, company='侠客测试公司', name='张经理', contact='test@example.invalid',
            challenge='需求说明', source='brief', brief='完整简报', stage='成长')


class DeliveryTests(unittest.TestCase):
    def test_mapping(self):
        p = payload_for(LEAD, OWNER)
        self.assertEqual(p['source'], '官网')
        self.assertEqual(p['ownerUserId'], OWNER)
        self.assertEqual(p['companyName'], LEAD['company'])
        self.assertIn(ID, p['summary'])
        self.assertIn('/admin', p['summary'])

    def test_lost_ack_retries_same_command_without_duplicate(self):
        commands = []; writes = {}; acknowledgements = []
        def api(url, headers, payload=None):
            if url.endswith('/pending'):
                return {'items': [LEAD] if not acknowledgements else []}
            if url.endswith('/ack'):
                if len(commands) == 1:
                    raise TimeoutError()
                acknowledgements.append(payload)
                return {'ok': True}
            self.assertNotIn('Authorization', headers)
            key = headers['Idempotency-Key']
            commands.append((key, payload))
            writes[key] = payload
            return {'id': ID}
        args = ('https://site.test', 'http://core', 'site-secret', 'core-secret', OWNER)
        self.assertEqual(synchronize(*args, request=api), (0, 1))
        self.assertEqual(synchronize(*args, request=api), (1, 0))
        self.assertEqual(commands[0], commands[1])
        self.assertEqual(len(writes), 1)
        self.assertEqual(synchronize(*args, request=api), (0, 0))

    def test_core_failure_does_not_ack(self):
        def api(url, headers, payload=None):
            if url.endswith('/pending'):
                return {'items': [LEAD]}
            self.assertFalse(url.endswith('/ack'))
            raise TimeoutError()
        self.assertEqual(synchronize('https://site', 'http://core', 's', 'c', OWNER, api), (0, 1))


if __name__ == '__main__':
    unittest.main()
