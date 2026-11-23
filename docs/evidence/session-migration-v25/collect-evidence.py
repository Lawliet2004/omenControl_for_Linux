#!/usr/bin/env python3
"""Publish immutable source and runtime evidence for scope migration recovery."""
import hashlib
import json
from pathlib import Path
import re
import shutil

ROOT=Path(__file__).resolve().parents[1]
RAW=ROOT/'target/session-migration-v25'
OUT=ROOT/'docs/evidence/session-migration-v25'

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
    assert not OUT.exists(),'published evidence cannot be overwritten'
    vm=read(RAW/'vm/vm-systemd.json');package=read(RAW/'vm/package-validation.json')
    assert vm['result']=='passed' and vm['vm_exited'] and vm['temporary_files_removed']
    assert len(vm['checks_passed'])==40 and vm['flatpak_migration_phase_expected']=='interrupted'
    migration=vm['flatpak_scope_probe']['managed_migration'];session=migration['sessions']['sessions'][0]
    assert session['phase']=='interrupted' and not session['profile_held'] and 'moved outside' in session['detail']
    assert migration['profile_while_sandbox_alive']=='balanced'
    assert package['vm_equivalence']['runtime_payload_identical'] and package['vm_equivalence']['maintainer_scripts_and_control_identical']
    assert package['systemd_validation']['exit_code']==0 and all(package['checks'].values())
    final=ROOT/package['package'];assert sha(final)==package['sha256']=='c84e5e60bc2e0b7254cf0430e68030c0e2681ff13913c39336621fe7068f5e54'
    builds={};native={};scope={}
    for kind in ['debug','release']:
        builds[kind]=sum(map(int,re.findall(r'test result: ok\. (\d+) passed',(RAW/f'rust-{kind}.log').read_text())))
        assert builds[kind]==276
        native[kind]=read(RAW/f'native-{kind}/managed-games-native.json')
        scope[kind]=read(RAW/f'scope-native-{kind}/game-session-native.json')
        assert native[kind]['result']=='passed' and len(native[kind]['checks_passed'])==21 and native[kind]['cleanup_complete']
        assert scope[kind]['result']=='passed' and len(scope[kind]['checks_passed'])==12 and scope[kind]['cleanup_complete']
    helper=read(RAW/'candidate-manifest.json')
    assert sha(Path(helper['helper']))==helper['helper_sha256']==vm['managed_session_fixture']['helper_sha256']
    assert sha(ROOT/'crates/cli/tests/managed_games_native.rs')==helper['helper_source_sha256']
    OUT.mkdir(parents=True)
    copy(Path(__file__),'collect-evidence.py')
    previous=ROOT/'docs/evidence/flatpak-scope-probe-v24/source-manifest.json'
    sources=[];changed=[]
    for item in read(previous):
        digest=sha(ROOT/item['path']);sources.append({'path':item['path'],'sha256':digest})
        if digest!=item['sha256']:changed.append(item['path'])
        copy(ROOT/item['path'],Path('sources')/item['path'])
    assert len(sources)==132 and set(changed)=={
        'crates/cli/tests/managed_games_native.rs','crates/daemon/src/game_sessions.rs',
        'crates/hardware/src/game_scope.rs','packaging/build-deb.sh','packaging/debian/changelog'}
    write('source-manifest.json',sources)
    tests=[]
    for name in ['systemd_vm.py','systemd_vm_inner.py','coarse_recovery_vm_inner.py',
            'nvidia_reader_vm_inner.py','managed_games_vm_inner.py','flatpak_scope_vm_inner.py','package.py']:
        relative='tests/'+name;copy(ROOT/relative,Path('sources')/relative)
        tests.append({'path':relative,'sha256':sha(ROOT/relative)})
    write('test-manifest.json',tests)
    documents=[]
    for name in ['SESSION_SCOPE_MIGRATION_2026-10-28.md','APPLICATION_AUDIT_IMPLEMENTATION.md',
            'IMPLEMENTATION_STATUS.md','CONTINUATION_CHECKLIST.md','TEST_REPORT.md',
            'MANAGED_GAME_SESSIONS.md','GAME_LAUNCHERS.md']:
        relative='docs/'+name;copy(ROOT/relative,Path('reports')/relative)
        documents.append({'path':relative,'sha256':sha(ROOT/relative)})
    write('documentation-manifest.json',documents)
    for name in ['rust-debug.log','rust-release.log','clippy.log','fmt.log',
            'native-debug-artifacts.jsonl','native-debug-build.log','native-release-artifacts.jsonl',
            'native-release-build.log','candidate-build.log','final-build.log','package-verification.log',
            'candidate-manifest.json','vm-command.json','vm-console.log','scope-native-debug.log',
            'scope-native-release.log','host-preservation.json']:
        copy(RAW/name,Path('runs')/name)
    for folder in ['native-debug','native-release','scope-native-debug','scope-native-release','vm','vm-test-inputs']:
        tree(RAW/folder,Path('runs')/folder)
    prior=ROOT/'docs/evidence/flatpak-scope-probe-v24/validation-summary.json'
    assert sha(prior)=='1844e6c78308975f1dc8163ca0590cd8d1efee01927b30071b06d911b2e8d9da'
    copy(ROOT/'docs/evidence/flatpak-scope-probe-v24/inputs/repository-manifest.json','inputs/repository-manifest.json')
    copy(ROOT/'docs/evidence/flatpak-scope-probe-v24/inputs/repository-independent-verification.json','inputs/repository-independent-verification.json')
    copy(ROOT/'docs/evidence/flatpak-scope-probe-v24/inputs/upstream/pinned-source-verification.json','inputs/upstream-source-verification.json')
    (OUT/'README.md').write_text('''# Revision 25 scope migration recovery

The source fixes the original registered peer leaving its tracked scope. It
releases its profile hold and records Interrupted while the process remains
alive. This is not a managed Flatpak implementation or game-compatibility claim.

The candidate has 40 installed VM checks and two reboots. The final package's
runtime payload, maintainer scripts and control metadata are identical to that
candidate, with six finalized document payloads. See the package comparison and
reports/docs/SESSION_SCOPE_MIGRATION_2026-10-28.md for exact attribution.

Debug/release each pass 276 Rust, 21 native managed-session and 12 lower-level
scope/provider checks. The release native run uses the ordinary packaged CLI.
The current helper was selected from Cargo artifact JSON and retained at
target/session-migration-v25/artifacts/managed-games-native-release, with its
hash/source identity in runs/candidate-manifest.json. Large compiled helpers and
packages are retained there rather than duplicated in this evidence directory.

The seven Flatpak observations use the same pinned Ubuntu packages and explicit
local fixture as evidence/flatpak-scope-probe-v24. Upstream source/archive and
signature/package provenance remain in that immutable publication. This bundle
copies provenance manifests and records its exact summary hash. No host install,
system power-provider call or physical control write occurs. No new GUI, real
graphical authorization, real game, full accessibility or physical validation
is claimed. Earlier packages and evidence retain their attribution.
''')
    files=[{'path':str(p.relative_to(OUT)),'bytes':p.stat().st_size,'sha256':sha(p)}
        for p in sorted(OUT.rglob('*')) if p.is_file()]
    summary={'result':'passed','revision':25,'scope_migration_reported_interrupted':True,
        'managed_flatpak_supported':False,'package':str(final.relative_to(ROOT)),
        'package_bytes':final.stat().st_size,'package_sha256':sha(final),
        'vm_tested_candidate_sha256':vm['package_sha256'],'vm_runtime_equivalence':package['vm_equivalence'],
        'rust_tests':builds,'native_managed_checks':{k:len(v['checks_passed']) for k,v in native.items()},
        'native_scope_provider_checks':{k:len(v['checks_passed']) for k,v in scope.items()},
        'installed_vm_checks':40,'flatpak_boundary_observations':7,
        'product_source_files':len(sources),'changed_product_build_inputs':changed,
        'current_release_helper_sha256':helper['helper_sha256'],
        'physical_hardware_writes':False,'host_installation':False,
        'new_gui_validation_claimed':False,'real_flatpak_game_tested':False,
        'prior_flatpak_evidence':str(prior.relative_to(ROOT)),'prior_flatpak_evidence_sha256':sha(prior),
        'host_preservation':read(RAW/'host-preservation.json'),'files':files}
    write('validation-summary.json',summary)
    print(json.dumps({'evidence_files':len(files),'summary_sha256':sha(OUT/'validation-summary.json'),
        'package_sha256':sha(final),'package_bytes':final.stat().st_size},indent=2))

if __name__=='__main__':main()
