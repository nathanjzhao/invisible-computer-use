#!/usr/bin/env python3
"""Refresh signed Linux metadata on ephemeral CodeBuild; never persist signing keys."""
import argparse
import datetime as dt
from email.utils import parsedate_to_datetime, format_datetime
import hashlib
import json
import os
from pathlib import Path
import resource
import re
import subprocess
import tempfile
import urllib.request
import xml.etree.ElementTree as ET

BUCKET = 'icu-linux-release-463200133738-us-west-2'
BASE = 'https://d3sr5pcu8uoa7b.cloudfront.net'
FINGERPRINTS = {'F72633BD81FB499F681A798D2024879F2F32BC78', '40CFF89EFBAC72693CD207B68911048A88AB1348'}
APT = ['apt/dists/stable', 'apt/ubuntu-26.04/dists/ubuntu-26.04', 'apt/debian-13/dists/debian-13']
RPM = ['rpm/repodata', 'rpm/fedora/43/repodata', 'rpm/fedora/44/repodata']


def expiry(release, now):
    values = [line.split(':', 1)[1].strip() for line in release.decode().splitlines() if line.startswith('Valid-Until:')]
    if len(values) != 1:
        raise ValueError('Release must have exactly one Valid-Until')
    end = parsedate_to_datetime(values[0])
    if end.tzinfo is None or end - now < dt.timedelta(days=14):
        raise ValueError('signed metadata expires in under 14 days')
    return end.isoformat()


def renew(release, now):
    lines = release.decode().splitlines()
    for name, value in [('Date', now), ('Valid-Until', now + dt.timedelta(days=30))]:
        lines = [line for line in lines if not line.startswith(name + ':')]
        lines.insert(0, name + ': ' + format_datetime(value))
    output = ('\n'.join(lines) + '\n').encode()
    expiry(output, now)
    return output


def renew_rpm(body, now):
    """Update the revision without rewriting namespace prefixes or index data.

    librepo rejects the ns0-prefixed root emitted by ElementTree's default
    serializer. Keep the signed document's representation and change only the
    one supported revision field, after validating its structure.
    """
    document = ET.fromstring(body)
    namespace = '{http://linux.duke.edu/metadata/repo}'
    revisions = document.findall(namespace + 'revision')
    if document.tag != namespace + 'repomd' or len(revisions) != 1:
        raise ValueError('RPM metadata requires exactly one revision')
    pattern = rb'(<revision(?:\s[^>]*)?>)\s*([0-9]+)\s*(</revision\s*>)'
    matches = list(re.finditer(pattern, body))
    if len(matches) != 1 or matches[0].group(2).decode() != (revisions[0].text or '').strip():
        raise ValueError('RPM revision representation is ambiguous or unsupported')
    revision = str(int(now.timestamp())).encode()
    match = matches[0]
    return body[:match.start(2)] + revision + body[match.end(2):]


def run(args, data=None):
    result = subprocess.run(args, input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        # Arguments and input can contain sensitive process data; emit only the tool name.
        raise RuntimeError(Path(args[0]).name + ' failed: ' + str(result.returncode))
    return result.stdout, result.stderr


def main(refresh=False):
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    os.umask(0o077)
    if refresh:
        if len(Path('/proc/swaps').read_text().splitlines()) != 1:
            run(['swapoff', '-a'])
        if len(Path('/proc/swaps').read_text().splitlines()) != 1:
            raise RuntimeError('signing requires disabled swap')
    mount = run(['stat', '-f', '-c', '%T', '/dev/shm'])[0].strip()
    if mount != b'tmpfs':
        raise RuntimeError('signing home requires tmpfs')
    now = dt.datetime.now(dt.timezone.utc).replace(microsecond=0)
    s3 = None
    if refresh:
        import boto3
        session = boto3.Session(region_name='us-west-2')
        if session.client('sts').get_caller_identity()['Account'] != '463200133738':
            raise RuntimeError('wrong AWS account')
        s3 = session.client('s3')
    originals = {}
    def fetch(name):
        if s3:
            obj = s3.get_object(Bucket=BUCKET, Key='public/' + name)
            body = obj['Body'].read()
            originals[name] = obj['ETag']
            return body
        with urllib.request.urlopen(BASE + '/' + name, timeout=30) as response:
            return response.read()
    receipt = {'at': now.isoformat(), 'mode': 'refresh' if refresh else 'check', 'metadata': {}}
    with tempfile.TemporaryDirectory(prefix='icu-metadata-', dir='/dev/shm') as directory:
        home = Path(directory)
        gpg = ['gpg', '--homedir', directory, '--batch', '--yes', '--pinentry-mode', 'loopback']
        try:
            run(gpg + ['--import'], fetch('keys/repository.asc'))
            listing = run(gpg + ['--with-colons', '--fingerprint'])[0].decode()
            primary = set()
            want = False
            for line in listing.splitlines():
                fields = line.split(':')
                if fields[0] == 'pub': want = True
                elif fields[0] == 'fpr' and want:
                    primary.add(fields[9]); want = False
            if primary != FINGERPRINTS:
                raise ValueError('repository fingerprints differ')
            fingerprint = None
            if refresh:
                secret = json.loads(session.client('secretsmanager').get_secret_value(SecretId='icu-linux-release/repository')['SecretString'])
                fingerprint = secret['rotation_fingerprint']
                if fingerprint not in FINGERPRINTS:
                    raise ValueError('unapproved signing fingerprint')
                run(gpg + ['--import'], secret['private_key'].encode())
                del secret
            writes = {}
            def verify(body, signature):
                (home / 'body').write_bytes(body)
                (home / 'signature').write_bytes(signature)
                _, status = run(gpg + ['--status-fd', '2', '--verify', str(home / 'signature'), str(home / 'body')])
                if not any(('VALIDSIG ' + f).encode() in status for f in FINGERPRINTS):
                    raise ValueError('signature is not from the pinned repository keys')
            def sign(body, inline=False):
                return run(gpg + ['--local-user', fingerprint, '--armor', '--output', '-',
                                  '--clearsign' if inline else '--detach-sign'], body)[0]
            for root in APT:
                release = fetch(root + '/Release')
                verify(release, fetch(root + '/Release.gpg'))
                inline = fetch(root + '/InRelease')
                decoded, _ = run(gpg + ['--decrypt'], inline)
                if decoded != release:
                    raise ValueError('InRelease differs from detached Release')
                if refresh:
                    release = renew(release, now)
                    writes[root + '/Release'] = release
                    writes[root + '/Release.gpg'] = sign(release)
                    writes[root + '/InRelease'] = sign(release, True)
                    verify(release, writes[root + '/Release.gpg'])
                receipt['metadata'][root] = {'sha256': hashlib.sha256(release).hexdigest(), 'valid_until': expiry(release, now)}
            for root in RPM:
                body = fetch(root + '/repomd.xml')
                verify(body, fetch(root + '/repomd.xml.asc'))
                document = ET.fromstring(body)
                revision = document.find('{http://linux.duke.edu/metadata/repo}revision')
                if revision is None: raise ValueError('missing RPM revision')
                if refresh:
                    body = renew_rpm(body, now)
                    revision.text = str(int(now.timestamp()))
                    writes[root + '/repomd.xml'] = body
                    writes[root + '/repomd.xml.asc'] = sign(body)
                    verify(body, writes[root + '/repomd.xml.asc'])
                deadline = dt.datetime.fromtimestamp(int(revision.text), dt.timezone.utc) + dt.timedelta(days=30)
                # This job enforces freshness; DNF itself does not enforce expiry.
                rpm_expiry = expiry(('Valid-Until: ' + format_datetime(deadline) + '\n').encode(), now)
                receipt['metadata'][root] = {'sha256': hashlib.sha256(body).hexdigest(), 'valid_until': rpm_expiry}
            if refresh:
                # Refuse publication if a release changed metadata while this job signed.
                for name in writes:
                    if s3.head_object(Bucket=BUCKET, Key='public/' + name)['ETag'] != originals[name]:
                        raise RuntimeError('concurrent repository publication')
                for name, body in writes.items():
                    s3.put_object(Bucket=BUCKET, Key='public/' + name, Body=body,
                                  IfMatch=originals[name], CacheControl='max-age=60')
                session.client('cloudfront').create_invalidation(DistributionId='EZ1DAE186YRZ5',
                    InvalidationBatch={'CallerReference': 'metadata-' + now.isoformat(),
                                       'Paths': {'Quantity': 2, 'Items': ['/apt/*', '/rpm/*']}})
        finally:
            subprocess.run(['gpgconf', '--homedir', directory, '--kill', 'all'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    receipt['signing_tmpfs_removed'] = not home.exists()
    print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--refresh', action='store_true')
    main(parser.parse_args().refresh)
