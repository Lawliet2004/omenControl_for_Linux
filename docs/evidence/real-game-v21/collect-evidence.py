#!/usr/bin/env python3
"""Publish this real-game validation once, without replacing prior evidence."""
import datetime
import hashlib
import json
import lzma
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[1]
RAW = ROOT / 'target/real-game-v21'
OUT = ROOT / 'docs/evidence/real-game-v21'


def sha(path):
    return hashlib.file_digest(path.open('rb'), 'sha256').hexdigest()


def copy(source, relative):
    target = OUT / relative
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, target)


def main():
    assert not OUT.exists(), 'published evidence is immutable'
    vm = json.loads((RAW / 'game-vm-confirmation/vm-systemd.json').read_text())
    assert vm['result'] == 'passed' and vm['real_game']['result'] == 'passed'
    assert vm['vm_exit_code'] == 0 and vm['temporary_files_removed']
    assert len(vm['real_game']['checks_passed']) == 6 and len(vm['checks_passed']) == 39
    assert sha(ROOT / 'tests/real_game_vm_inner.py') == vm['real_game']['source_sha256']
    package = ROOT / 'target/debian/omen-linux_0.1.0-21_amd64.deb'
    assert sha(package) == vm['package_sha256'] == '25964019083530e3c3e567dca7edd13cb4eebe1ce0ad2eea8aacc7f301ae3edf'
    OUT.mkdir(parents=True)
    copy(Path(__file__), 'collect-evidence.py')
    for folder in ['game-vm', 'game-vm-retry', 'game-vm-ready', 'game-vm-final', 'game-vm-confirmation', 'resolution']:
        for path in sorted((RAW / folder).rglob('*')):
            if path.is_file():
                copy(path, Path(folder) / path.relative_to(RAW / folder))
    for name in ['game-vm-command.json', 'resolution-command.json', 'requested-packages.json', 'host-preservation.json']:
        copy(RAW / name, name)
    for folder in ['metadata', 'complete']:
        copy(RAW / folder / 'repository-manifest.json', folder + '/repository-manifest.json')
    for path in sorted((RAW / 'indices').iterdir()):
        target = OUT / 'indices' / (path.name + '.xz' if path.name.endswith('_Packages') else path.name)
        target.parent.mkdir(exist_ok=True)
        if path.name.endswith('_Packages'):
            with lzma.open(target, 'wb', preset=6) as stream:
                stream.write(path.read_bytes())
        else:
            shutil.copyfile(path, target)
    sources = [Path('tests') / name for name in ['systemd_vm.py', 'systemd_vm_inner.py',
        'coarse_recovery_vm_inner.py', 'nvidia_reader_vm_inner.py', 'managed_games_vm_inner.py', 'real_game_vm_inner.py']]
    sources += [Path('scripts/prepare-offline-vm-repository.py'), Path('crates/cli/tests/managed_games_native.rs')]
    for source in sources:
        copy(ROOT / source, Path('sources') / source)
    reports = ['REAL_GAME_VALIDATION_2026-10-28.md', 'APPLICATION_AUDIT_IMPLEMENTATION.md',
        'IMPLEMENTATION_STATUS.md', 'MANAGED_GAME_SESSIONS.md', 'CONTINUATION_CHECKLIST.md', 'VM_VALIDATION.md', 'TEST_REPORT.md']
    for report in reports:
        copy(ROOT / 'docs' / report, Path('reports') / report)
    copy(ROOT / 'README.md', 'reports/README.md')
    preservation = {}
    for historical in ['desktop-v21', 'deployment-v20', 'launcher-expansion']:
        directory = ROOT / 'docs/evidence' / historical
        report = json.loads((directory / 'validation-summary.json').read_text())
        for entry in report['evidence_files']:
            assert sha(directory / entry['path']) == entry['sha256'], (historical, entry['path'])
        preservation[historical] = {'validation_summary_sha256': sha(directory / 'validation-summary.json'),
            'evidence_files_rehashed': len(report['evidence_files'])}
    evidence = [{'path': str(path.relative_to(OUT)), 'sha256': sha(path), 'bytes': path.stat().st_size}
        for path in sorted(OUT.rglob('*')) if path.is_file()]
    summary = {'result': 'passed', 'recorded_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'version': '0.1.0-21', 'package': str(package.relative_to(ROOT)), 'package_sha256': sha(package),
        'product_runtime_unchanged': True, 'fresh_vm_checks': len(vm['checks_passed']),
        'real_game_checks': len(vm['real_game']['checks_passed']),
        'game': {'name': vm['real_game']['real_game'], 'installed_kib': vm['real_game']['installed_game_kib'],
            'versions': vm['real_game']['versions'], 'native_launcher': 'Lutris Linux runner'},
        'guest_fullscreen': 'Plasma Wayland with an Xwayland/SDL game; guest uinput only',
        'private_profile_provider': True, 'host_package_or_service_installation': False,
        'physical_hardware_writes': False, 'english_only': True,
        'historical_evidence_preserved': preservation,
        'failed_runs_preserved': ['game-vm', 'game-vm-retry', 'game-vm-ready', 'game-vm-final'],
        'limits': ['One native free game and Linux runner; no store-account, Steam/Proton, Heroic or Flatpak compatibility claim.',
            'Private PPD fixture and production session/service code under installed sandbox; no physical profile write.',
            'Game fullscreen transition does not establish OMEN overlay placement, global shortcuts or physical input.'],
        'evidence_files': evidence}
    (OUT / 'validation-summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps({k:v for k,v in summary.items() if k != 'evidence_files'}, indent=2))


if __name__ == '__main__':
    main()
