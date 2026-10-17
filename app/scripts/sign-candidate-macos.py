#!/usr/bin/env python3
"""Sign and notarize a frozen acceptance candidate without rebuilding or launching it."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import secrets
import shlex
import shutil
import subprocess
import tempfile


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--candidate', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--issuer', required=True)
    parser.add_argument('--identity', default='Developer ID Application: Nathan Zhao (G7P6939437)')
    parser.add_argument('--key-id', default='G63MAFDGA5')
    args = parser.parse_args()
    candidate = args.candidate.resolve()
    args.out = args.out.resolve()
    if args.out.exists():
        raise SystemExit('Use a fresh output directory')
    manifest = json.loads((candidate / 'build-manifest.json').read_text())
    spec = importlib.util.spec_from_file_location('prepare', Path(__file__).with_name('prepare-e2e-macos.py'))
    prepare = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(prepare)
    if not manifest.get('frozen_local_sources') or manifest['source_sha256'] != prepare.source_hashes(candidate / 'source'):
        raise SystemExit('Candidate source binding failed')
    bundle = candidate / 'Copilot.app'
    native = bundle / 'Contents/MacOS'
    for filename, key in [('app', 'app_sha256'), ('cua-driver', 'driver_sha256'),
                          ('taiku-observer', 'observer_sha256'), ('taiku-observer-audio-helper', 'audio_helper_sha256')]:
        if digest(native / filename) != manifest[key]:
            raise SystemExit('Candidate binary binding failed: ' + filename)
    entitlements = candidate / 'source/app/src-tauri/entitlements.mac.plist'
    expected = plistlib.loads(entitlements.read_bytes())
    for key in ['com.apple.security.automation.apple-events', 'com.apple.security.device.audio-input']:
        if expected.get(key) is not True:
            raise SystemExit('Candidate lacks required entitlement: ' + key)
    info = plistlib.loads((bundle / 'Contents/Info.plist').read_bytes())
    for key in ['NSMicrophoneUsageDescription', 'NSScreenCaptureUsageDescription', 'NSAppleEventsUsageDescription']:
        if not info.get(key):
            raise SystemExit('Candidate lacks permission explanation: ' + key)
    args.out.mkdir(parents=True)
    signed = args.out / 'Copilot.app'
    shutil.copytree(bundle, signed)
    log = (args.out / 'signing.log').open('wb')

    def run(command, *, env=None, timeout=120):
        result = subprocess.run(command, capture_output=True, env=env, timeout=timeout)
        # Arguments include temporary secrets for security(1); never log argv.
        log.write(result.stdout + result.stderr)
        log.flush()
        if result.returncode:
            raise RuntimeError(Path(command[0]).name + ' failed; inspect signing.log')
        return result.stdout

    key = Path.home() / '.tauri/signing-request/developerID_application.key'
    certificate = Path.home() / 'Downloads/developerID_application.cer'
    notary_key = Path.home() / ('.appstoreconnect/private_keys/AuthKey_' + args.key_id + '.p8')
    with tempfile.TemporaryDirectory(prefix='signing-', dir=args.out) as temp:
        temp = Path(temp)
        keychain = temp / 'candidate.keychain-db'
        password = secrets.token_hex(24)
        env = dict(os.environ, ICU_P12_PASSWORD=password)
        created = False
        try:
            run(['openssl', 'x509', '-inform', 'der', '-in', str(certificate), '-out', str(temp / 'cert.pem')])
            # Compare key material in memory; do not add it to logs.
            cert_public = subprocess.check_output(['openssl', 'x509', '-in', str(temp / 'cert.pem'), '-noout', '-pubkey'])
            key_public = subprocess.check_output(['openssl', 'pkey', '-in', str(key), '-pubout'])
            if cert_public != key_public:
                raise RuntimeError('Developer ID certificate and private key do not match')
            run(['curl', '-fsSL', '-o', str(temp / 'g2.der'), 'https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer'])
            run(['openssl', 'x509', '-inform', 'der', '-in', str(temp / 'g2.der'), '-out', str(temp / 'g2.pem')])
            run(['openssl', 'pkcs12', '-export', '-inkey', str(key), '-in', str(temp / 'cert.pem'),
                 '-certfile', str(temp / 'g2.pem'), '-name', args.identity, '-keypbe', 'PBE-SHA1-3DES',
                 '-certpbe', 'PBE-SHA1-3DES', '-macalg', 'sha1', '-passout', 'env:ICU_P12_PASSWORD', '-out', str(temp / 'id.p12')], env=env)
            run(['security', 'create-keychain', '-p', password, str(keychain)])
            created = True
            run(['security', 'unlock-keychain', '-p', password, str(keychain)])
            run(['security', 'import', str(temp / 'id.p12'), '-k', str(keychain), '-P', password, '-T', '/usr/bin/codesign'])
            run(['security', 'set-key-partition-list', '-S', 'apple-tool:,apple:,codesign:', '-s', '-k', password, str(keychain)])
            # Trust-chain lookup also uses the search list, even with --keychain.
            # Merge the live list and let delete-keychain remove only our entry.
            search = shlex.split(subprocess.check_output(['security', 'list-keychains', '-d', 'user'], text=True))
            run(['security', 'list-keychains', '-d', 'user', '-s', *search, str(keychain)])
            run(['security', 'find-identity', '-v', '-p', 'codesigning', str(keychain)])
            audio = temp / 'audio.plist'
            audio.write_bytes(plistlib.dumps({'com.apple.security.device.audio-input': True}))
            for filename in ['cua-driver', 'taiku-observer', 'taiku-observer-audio-helper']:
                command = ['codesign', '--force', '--sign', args.identity, '--keychain', str(keychain), '--options', 'runtime', '--timestamp']
                if filename.startswith('taiku-observer'):
                    command += ['--entitlements', str(audio)]
                run(command + [str(signed / 'Contents/MacOS' / filename)])
            run(['codesign', '--force', '--sign', args.identity, '--keychain', str(keychain), '--options', 'runtime', '--timestamp',
                 '--entitlements', str(entitlements), str(signed)])
        finally:
            if created:
                # Delete only this run's keychain. Never restore a stale global search list.
                run(['security', 'delete-keychain', str(keychain)])
    run(['codesign', '--verify', '--deep', '--strict', str(signed)])
    actual = plistlib.loads(run(['codesign', '-d', '--entitlements', ':-', str(signed)]))
    if actual != expected:
        raise RuntimeError('Signed entitlements differ from frozen sources')
    archive = args.out / 'Copilot-notarization.zip'
    run(['ditto', '-c', '-k', '--keepParent', str(signed), str(archive)])
    receipt = run(['xcrun', 'notarytool', 'submit', str(archive), '--key', str(notary_key), '--key-id', args.key_id,
                   '--issuer', args.issuer, '--output-format', 'json', '--wait', '--timeout', '30m'], timeout=1900)
    (args.out / 'notarization.json').write_bytes(receipt)
    result = json.loads(receipt)
    if result.get('status') != 'Accepted':
        raise RuntimeError('Notarization did not accept candidate')
    run(['xcrun', 'stapler', 'staple', str(signed)])
    run(['xcrun', 'stapler', 'validate', str(signed)])
    run(['spctl', '-a', '-vv', '-t', 'exec', str(signed)])
    run(['codesign', '--verify', '--deep', '--strict', str(signed)])
    run(['ditto', '-c', '-k', '--keepParent', str(signed), str(args.out / 'Copilot.zip')])
    manifest.update(signing='Developer ID, notarized, stapled', notarization_id=result['id'],
                    status='signed_not_live_tested', entitlements=actual,
                    unsigned_manifest_sha256=digest(candidate / 'build-manifest.json'),
                    signing_script_sha256=digest(Path(__file__)), package_sha256=digest(args.out / 'Copilot.zip'))
    for filename, name in [('app', 'app'), ('cua-driver', 'driver'), ('taiku-observer', 'observer'), ('taiku-observer-audio-helper', 'audio_helper')]:
        manifest[name + '_sha256'] = digest(signed / 'Contents/MacOS' / filename)
    (args.out / 'signed-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    log.close()
    print(json.dumps({key: manifest[key] for key in ['status', 'signing', 'notarization_id', 'package_sha256']}))


if __name__ == '__main__':
    main()
