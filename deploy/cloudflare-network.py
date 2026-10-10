#!/usr/bin/env python3
"""Plan/apply only AUDENIQ's named transport/compression rules, preserving other rules."""
import argparse
import json
import os
from pathlib import Path
import urllib.error
import urllib.request


def api(method, path, payload=None):
    body = None if payload is None else json.dumps(payload).encode()
    request = urllib.request.Request(
        'https://api.cloudflare.com/client/v4' + path, body,
        {'Authorization': 'Bearer ' + os.environ['CLOUDFLARE_API_TOKEN'],
         'Content-Type': 'application/json'}, method=method,
    )
    # Never follow a redirect carrying the bearer token.
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, headers, newurl):
            return None
    try:
        with urllib.request.build_opener(NoRedirect()).open(request, timeout=30) as response:
            result = json.load(response)
    except urllib.error.HTTPError as error:
        raise SystemExit(f'Cloudflare {method} failed: HTTP {error.code}') from None
    if not result.get('success'):
        raise SystemExit('Cloudflare rejected request: ' + json.dumps(result.get('errors', [])))
    return result['result']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--zone-id', required=True)
    parser.add_argument('--apply', action='store_true', help='write the reviewed rules; default only previews')
    args = parser.parse_args()
    if not (len(args.zone_id) == 32 and all(c in '0123456789abcdef' for c in args.zone_id)):
        parser.error('zone ID must be 32 lowercase hexadecimal characters')
    zone = '/zones/' + args.zone_id
    if api('GET', zone)['name'] != 'audeniq.com':
        raise SystemExit('Expected audeniq.com zone')
    desired = json.loads(Path(__file__).with_suffix('.json').read_text())
    plans = []
    # Fetch every phase before making any mutation. An unavailable permission
    # is detected before touching either phase; the API validates quotas on writes.
    for phase, own_rules in desired.items():
        existing = api('GET', zone + '/rulesets')
        entry = next((r for r in existing if r['phase'] == phase and r['kind'] == 'zone'), None)
        current = api('GET', zone + '/rulesets/' + entry['id']) if entry else None
        refs = {r['ref'] for r in own_rules}
        rules = []
        for rule in (current or {}).get('rules', []):
            if rule.get('ref') not in refs:
                # Cloudflare response-only timestamps do not belong in writes.
                rules.append({k: v for k, v in rule.items() if k not in {'last_updated', 'version'}})
        # Put the HTTP block first, so an existing skip cannot bypass it.
        # Compression rules use the last match, so append ours there.
        rules = own_rules + rules if phase == 'http_request_firewall_custom' else rules + own_rules
        payload = {'name': (current or {}).get('name', 'AUDENIQ ' + phase),
                   'kind': 'zone', 'phase': phase, 'rules': rules}
        path = zone + '/rulesets' + ('/' + entry['id'] if entry else '')
        plans.append(('PUT' if entry else 'POST', path, payload))
    print(json.dumps({'apply': args.apply, 'plans': [p[2] for p in plans]}, indent=2))
    if args.apply:
        for method, path, payload in plans:
            applied = api(method, path, payload)
            readback = api('GET', zone + '/rulesets/' + applied['id'])
            actual = {r.get('ref'): r for r in readback['rules']}
            for wanted in desired[payload['phase']]:
                if any(actual.get(wanted['ref'], {}).get(k) != v for k, v in wanted.items()):
                    raise SystemExit('Cloudflare readback differs from the requested rule')
        print('Applied and verified AUDENIQ HTTPS and compression rules')


if __name__ == '__main__':
    main()
