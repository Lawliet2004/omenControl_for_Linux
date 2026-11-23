#!/usr/bin/env python3
"""Build a signed Ubuntu 26.04 file repository; never install host packages.

Copy the three pockets' InRelease and selected binary-amd64 Packages indexes
from an existing APT lists directory first. Without --resolution this prepares the
resolver's input; --resolution selects the exact archives reported by guest APT.
"""
import argparse
import concurrent.futures
import hashlib
import json
from pathlib import Path
import shlex
import shutil
import subprocess
import tarfile
import urllib.parse


def sha(path):
    return hashlib.file_digest(path.open('rb'), 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--indices', type=Path, required=True)
    parser.add_argument('--destination', type=Path, required=True)
    parser.add_argument('--resolution', type=Path)
    parser.add_argument('--component', action='append', choices=['main', 'universe', 'multiverse'], default=[])
    parser.add_argument('--cache', type=Path, action='append', default=[])
    parser.add_argument('--mirror', action='append', default=[])
    parser.add_argument('--download-timeout', type=int, default=120)
    args = parser.parse_args()
    assert 10 <= args.download_timeout <= 900
    base = args.destination.resolve()
    repo = base / 'repository'
    base.mkdir(parents=True, exist_ok=True)
    packages, inputs, paths = {}, [], []
    for suite in ['resolute', 'resolute-updates', 'resolute-security']:
        releases = list(args.indices.glob('*_dists_' + suite + '_InRelease'))
        assert len(releases) == 1, ('select exactly one index per pocket', suite)
        release = releases[0]
        signature = subprocess.run(['gpgv', '--status-fd', '1', '--keyring',
            '/usr/share/keyrings/ubuntu-archive-keyring.gpg', str(release)], capture_output=True, text=True, check=True)
        prefix = release.name.removesuffix('_InRelease')
        signed = repo / 'dists' / suite / 'InRelease'
        signed.parent.mkdir(parents=True, exist_ok=True)
        signed.write_bytes(release.read_bytes())
        paths.append(signed)
        component_hashes = {}
        for component in args.component or ['main']:
            stem = prefix + '_' + component + '_binary-amd64_Packages'
            candidates = [args.indices / (stem + suffix) for suffix in ['', '.lz4', '.xz', '.gz']]
            index = next(path for path in candidates if path.is_file())
            data = subprocess.check_output(['/usr/lib/apt/apt-helper', 'cat-file', str(index)])
            record = next(line.split() for line in release.read_text().split('SHA256:\n', 1)[1].splitlines()
                if line.strip().endswith(component + '/binary-amd64/Packages'))
            assert len(data) == int(record[1]) and hashlib.sha256(data).hexdigest() == record[0]
            target = repo / 'dists' / suite / component / 'binary-amd64/Packages'
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            paths.append(target)
            component_hashes[component] = record[0]
            for paragraph in data.decode().split('\n\n'):
                fields = dict(line.split(': ', 1) for line in paragraph.splitlines()
                    if ': ' in line and not line.startswith(' '))
                if all(key in fields for key in ['Package', 'Version', 'Filename', 'SHA256', 'Size']):
                    name = fields['Filename']
                    assert name.startswith('pool/') and '..' not in Path(name).parts and name.endswith('.deb')
                    entry = {key: fields[key] for key in ['Package', 'Version', 'Architecture', 'Filename', 'SHA256', 'Size']}
                    assert name not in packages or packages[name]['SHA256'] == entry['SHA256']
                    packages[name] = entry
        inputs.append({'suite': suite, 'InRelease_sha256': sha(release), 'component_SHA256': component_hashes,
            'signature_status': signature.stdout.splitlines()})
    required = {}
    if args.resolution:
        for line in args.resolution.read_text().splitlines():
            if not line.startswith("'file:/var/tmp/omen-apt-repository/pool/"):
                continue
            fields = shlex.split(line)
            name = urllib.parse.unquote(fields[0].split('/omen-apt-repository/', 1)[1])
            entry = packages[name]
            assert int(entry['Size']) == int(fields[2])
            required[name] = entry
        assert required, 'guest resolution contains no repository archives'
    cache = {}
    for directory in args.cache:
        for path in directory.rglob('*.deb'):
            if path.is_file():
                cache.setdefault(path.stat().st_size, []).append(path)
    mirrors = args.mirror or ['https://archive.ubuntu.com/ubuntu/']
    for mirror in mirrors:
        url = urllib.parse.urlsplit(mirror)
        assert url.scheme == 'https' and url.hostname and not url.username and not url.password and not url.query and not url.fragment

    def fetch(entry):
        dest = repo / entry['Filename']
        dest.parent.mkdir(parents=True, exist_ok=True)
        size, expected = int(entry['Size']), entry['SHA256']
        for path in [dest, *cache.get(size, [])]:
            if path.is_file() and path.stat().st_size == size and sha(path) == expected:
                if path != dest:
                    shutil.copyfile(path, dest)
                return dict(entry, origin={'cache': str(path)})
        errors = []
        for mirror in mirrors:
            url = mirror.rstrip('/') + '/' + urllib.parse.quote(entry['Filename'], safe='/')
            partial = dest.with_name(dest.name + '.partial')
            if partial.exists() and partial.stat().st_size >= size:
                partial.unlink()
            call = subprocess.run(['curl', '--fail', '--location', '--silent', '--show-error',
                '--connect-timeout', '10', '--max-time', str(args.download_timeout), '--continue-at', '-',
                '--output', str(partial), url], capture_output=True, text=True)
            if call.returncode == 0 and partial.stat().st_size == size and sha(partial) == expected:
                partial.rename(dest)
                print('Verified archive:', entry['Package'], size, flush=True)
                return dict(entry, origin={'download': url})
            if partial.exists() and partial.stat().st_size == size and sha(partial) != expected:
                partial.unlink()
            errors.append({'url': url, 'exit_code': call.returncode, 'error': call.stderr.strip()})
        raise RuntimeError(str((entry['Package'], errors)))

    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
        verified = list(pool.map(fetch, required.values()))
    paths.extend(repo / entry['Filename'] for entry in verified)
    archive = base / ('complete-repository.tar' if args.resolution else 'metadata-repository.tar')
    with tarfile.open(archive, 'w') as output:
        for path in sorted(paths):
            info = output.gettarinfo(str(path), str(path.relative_to(repo)))
            info.uid = info.gid = 0
            info.mode, info.mtime = 0o644, 0
            with path.open('rb') as stream:
                output.addfile(info, stream)
    manifest = {'archive': str(archive), 'archive_bytes': archive.stat().st_size, 'sha256': sha(archive),
        'metadata_only': not bool(args.resolution), 'signed_indices': inputs, 'packages': verified,
        'host_packages_installed': False, 'guest_APT_verifies_again': True}
    (base / 'repository-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps({'archive': str(archive), 'sha256': manifest['sha256'], 'packages': len(verified)}, indent=2))


if __name__ == '__main__':
    main()
