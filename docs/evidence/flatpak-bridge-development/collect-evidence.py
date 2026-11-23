#!/usr/bin/env python3
"""Publish immutable development sources and explicitly attributed VM overlays."""
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile

ROOT=Path(__file__).resolve().parents[1]
RAW=ROOT/'target/flatpak-bridge-development'
OUT=ROOT/'docs/evidence/flatpak-bridge-development'

def sha(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def read(path):return json.loads(path.read_text())
def copy(source,relative):
    target=OUT/relative
    assert source.is_file() and not source.is_symlink() and not target.exists(),(source,target)
    target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(source,target)
def tree(source,relative):
    for path in sorted(source.rglob('*')):
        if path.is_file():copy(path,Path(relative)/path.relative_to(source))
def write(relative,value):
    target=OUT/relative;assert not target.exists();target.parent.mkdir(parents=True,exist_ok=True)
    target.write_text(json.dumps(value,indent=2)+'\n')

def main():
    assert not OUT.exists(),'published evidence must never be overwritten'
    core=read(RAW/'core-attempt-2/runtime/vm-systemd.json')
    wrapper=read(RAW/'wrapper-attempt-3/runtime/vm-systemd.json')
    for data,count in [(core,43),(wrapper,40)]:
        assert data['result']=='passed' and len(data['checks_passed'])==count
        assert data['vm_exited'] and data['temporary_files_removed']
    handoff=core['flatpak_scope_probe']['bridge_development']
    flow=wrapper['flatpak_wrapper_probe']
    assert handoff['before']==handoff['after'] and handoff['profile_after_host_bridge_exit']=='performance'
    assert flow['result']=='passed' and len(flow['checks_passed'])==7
    assert flow['installed_cli_restored'] and flow['fixture_cleanup_complete']
    captured=flow['detached_observation']['output']
    assert re.search(r'^LAUNCHER_CWD=(.+)$',captured,re.M).group(1)==re.search(r'^GAME_CWD=(.+)$',captured,re.M).group(1)
    binary=RAW/'artifacts/omen-cli-2'
    assert sha(binary)==handoff['cli_sha256']==flow['development_cli_sha256']=='3bd9abe6ae01df87ae1363ed7834b5eebd802ef0ba0ca80de3adfe1b9904600f'
    assert sha(ROOT/'packaging/omen-flatpak-game')==flow['wrapper_sha256']
    builds={}
    for profile in ['debug','release']:
        builds[profile]=sum(map(int,re.findall(r'test result: ok\. (\d+) passed',(RAW/f'rust-{profile}-2.log').read_text())))
        assert builds[profile]==277
    OUT.mkdir(parents=True)
    copy(Path(__file__),'collect-evidence.py')
    copy(ROOT/'LICENSE','LICENSE')
    copy(binary,'artifacts/omen-cli-development')
    base=ROOT/'target/debian/omen-linux_0.1.0-25_amd64.deb'
    assert sha(base)==core['package_sha256']==wrapper['package_sha256']=='c84e5e60bc2e0b7254cf0430e68030c0e2681ff13913c39336621fe7068f5e54'
    with tarfile.open(fileobj=io.BytesIO(subprocess.check_output(['dpkg-deb','--fsys-tarfile',str(base)]))) as archive:
        raw=archive.extractfile('./usr/share/doc/omen-linux/DEPENDENCY_NOTICES.md.gz').read()
        assert gzip.decompress(raw)
        (OUT/'DEPENDENCY_NOTICES.md.gz').write_bytes(raw)
    baseline=ROOT/'docs/evidence/session-migration-v25/source-manifest.json'
    paths={item['path'] for item in read(baseline)}
    paths.update(['crates/client/src/flatpak_bridge.rs','packaging/omen-flatpak-game'])
    sources=[]
    for relative in sorted(paths):
        copy(ROOT/relative,Path('sources')/relative)
        sources.append({'path':relative,'sha256':sha(ROOT/relative)})
    assert len(sources)==134
    write('source-manifest.json',sources)
    tests=[]
    for relative in ['tests/systemd_vm.py','tests/systemd_vm_inner.py','tests/coarse_recovery_vm_inner.py',
            'tests/nvidia_reader_vm_inner.py','tests/managed_games_vm_inner.py','tests/flatpak_scope_vm_inner.py',
            'tests/flatpak_wrapper_vm_inner.py','scripts/prepare-offline-vm-repository.py']:
        copy(ROOT/relative,Path('sources')/relative);tests.append({'path':relative,'sha256':sha(ROOT/relative)})
    write('test-manifest.json',tests)
    documents=[]
    for name in ['FLATPAK_BRIDGE_DEVELOPMENT_2026-10-28.md','APPLICATION_AUDIT_IMPLEMENTATION.md',
            'IMPLEMENTATION_STATUS.md','CONTINUATION_CHECKLIST.md','TEST_REPORT.md']:
        relative='docs/'+name;copy(ROOT/relative,Path('reports')/relative)
        documents.append({'path':relative,'sha256':sha(ROOT/relative)})
    write('documentation-manifest.json',documents)
    for name in ['core-attempt-1','core-attempt-2','spawn-dependency-resolution',
            'wrapper-attempt-1','wrapper-attempt-2','wrapper-attempt-3']:
        tree(RAW/name,Path('runs')/name)
    for name in ['check.log','check-2.log','check-3.log','check-4.log','build-cli.log','build-cli-2.log',
            'rust-debug.log','rust-debug-2.log','rust-release-2.log','clippy.log','clippy-2.log',
            'fmt.log','fmt-2.log','spawn-repository-build.log','lock-comparison.json',
            'development-cli.json','host-preservation.json']:
        copy(RAW/name,Path('runs')/name)
    copy(RAW/'spawn-repository/repository-manifest.json','inputs/repository-manifest.json')
    references=[]
    for name,path in [('dbus-scope.c','src/core/dbus-scope.c'),('bus-unit-util.c','src/shared/bus-unit-util.c')]:
        source=RAW/'inputs'/name
        references.append({'url':'https://raw.githubusercontent.com/systemd/systemd/v259/'+path,
            'sha256':sha(source),'bytes':source.stat().st_size,'license':'LGPL-2.1-or-later',
            'used_as':'API reference only; not copied into the implementation'})
    write('inputs/systemd-source-references.json',references)
    (OUT/'README.md').write_text('''# Unreleased Flatpak bridge development

This is development code, not a revision 26 package or supported Flatpak GUI setup.
See reports/docs/FLATPAK_BRIDGE_DEVELOPMENT_2026-10-28.md for remaining requirements.

The core suite has 43 checks and the separate HostCommand suite 40 checks. Each
uses finalized revision 25 as the base, with a separately retained development
CLI. The HostCommand fixture overlays/restores only the guest CLI before reboot
checks; there is no host installation. Both providers are private fixtures.
The wrapper suite uses the installed root-service sandbox and normal polkit.

Failed transport/setup/cleanup runs and exact per-run source snapshots are
retained separately. Do not combine them into a passing run. No real game,
Flathub launcher, GUI, graphical authorization or physical validation is claimed.
The tested CLI is in artifacts/, with project license and dependency notices.
Large signed repository/cloud image inputs remain in target/ with exact hashes
in the run and repository manifests. Earlier published artifacts are unchanged.
''')
    files=[{'path':str(path.relative_to(OUT)),'bytes':path.stat().st_size,'sha256':sha(path)}
        for path in sorted(OUT.rglob('*')) if path.is_file()]
    summary={'result':'development_fixture_verified','release_delivered':False,'intended_revision':26,
        'automatic_flatpak_gui_enabled':False,'real_game_tested':False,'real_store_launcher_tested':False,
        'base_package':str(base.relative_to(ROOT)),'base_package_sha256':sha(base),
        'development_cli_sha256':sha(binary),'wrapper_sha256':flow['wrapper_sha256'],
        'rust_tests':builds,'core_vm_checks':43,'core_flatpak_observations':10,
        'hostcommand_vm_checks':40,'hostcommand_workflows':7,
        'sandbox_execution_context_preserved':handoff['before']==handoff['after'],
        'root_service_sandbox_and_polkit':True,'physical_hardware_writes':False,
        'source_files':134,'test_inputs':8,'documents':5,'host_preservation':read(RAW/'host-preservation.json'),
        'files':files}
    write('validation-summary.json',summary)
    print(json.dumps({'files':len(files),'summary_sha256':sha(OUT/'validation-summary.json'),
        'development_cli_sha256':sha(binary)},indent=2))

if __name__=='__main__':main()
