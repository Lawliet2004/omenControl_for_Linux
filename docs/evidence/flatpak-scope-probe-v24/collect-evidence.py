#!/usr/bin/env python3
"""Publish exact, immutable inputs/results for the revision 24 Flatpak probe."""
import hashlib
import json
from pathlib import Path
import shutil
import tarfile

ROOT=Path(__file__).resolve().parents[1]
RAW=ROOT/'target/flatpak-scope-probe'
OUT=ROOT/'docs/evidence/flatpak-scope-probe-v24'

def digest(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()

def read(path):return json.loads(path.read_text())

def copy(source,relative):
    target=OUT/relative
    assert source.is_file() and not source.is_symlink() and not target.exists(),(source,target)
    target.parent.mkdir(parents=True,exist_ok=True)
    shutil.copyfile(source,target)

def tree(source,relative):
    for path in sorted(source.rglob('*')):
        if path.is_file():copy(path,Path(relative)/path.relative_to(source))

def write(relative,value):
    target=OUT/relative;assert not target.exists();target.parent.mkdir(parents=True,exist_ok=True)
    target.write_text(json.dumps(value,indent=2)+'\n')

def main():
    assert not OUT.exists(),'published evidence must never be overwritten'
    final=read(RAW/'attempt-5/runtime/vm-systemd.json');probe=final['flatpak_scope_probe']
    assert final['result']=='passed' and final['vm_exited'] and final['temporary_files_removed']
    assert len(final['checks_passed'])==40 and len(probe['checks_passed'])==7
    assert probe['result']=='passed' and probe['fixture_cleanup_complete']
    assert probe['enter_observation']['return_code']==0 and probe['direct_exit_control']['return_code']==7
    assert probe['enter_signal']['return_code']==15 and probe['managed_migration']['profile_while_sandbox_alive']=='balanced'
    assert probe['entry_descendant']['profile_after_entry_parent_exit']=='performance'
    assert probe['entry_descendant']['final_session']['phase']=='ended'
    package=ROOT/'target/debian/omen-linux_0.1.0-24_amd64.deb'
    assert digest(package)==final['package_sha256']=='1a7eb732fa5c672c2e920396ccdf163b718bb008008bc4cedd69ac7f8bb226df'
    baseline=ROOT/'docs/evidence/notification-workflow-v24/source-manifest.json'
    sources=read(baseline);assert len(sources)==132
    for item in sources:assert digest(ROOT/item['path'])==item['sha256'],item['path']
    OUT.mkdir(parents=True)
    copy(Path(__file__),'collect-evidence.py')
    copy(baseline,'source-manifest.json')
    for item in sources:copy(ROOT/item['path'],Path('sources')/item['path'])
    tests=[]
    for relative in ['tests/systemd_vm.py','tests/flatpak_scope_vm_inner.py','scripts/prepare-offline-vm-repository.py']:
        copy(ROOT/relative,Path('sources')/relative)
        tests.append({'path':relative,'sha256':digest(ROOT/relative)})
    write('test-manifest.json',tests)
    documents=[]
    for name in ['FLATPAK_SESSION_BOUNDARIES_2026-10-28.md','APPLICATION_AUDIT_IMPLEMENTATION.md',
            'IMPLEMENTATION_STATUS.md','CONTINUATION_CHECKLIST.md','TEST_REPORT.md',
            'MANAGED_GAME_SESSIONS.md','GAME_LAUNCHERS.md']:
        relative='docs/'+name;copy(ROOT/relative,Path('reports')/relative)
        documents.append({'path':relative,'sha256':digest(ROOT/relative)})
    write('documentation-manifest.json',documents)
    # Keep every failed/earlier pass and the exact input version used for it.
    tree(RAW/'runtime','runs/attempt-1/runtime')
    tree(RAW/'initial-test-inputs','runs/attempt-1/test-inputs')
    copy(RAW/'vm-console.log','runs/attempt-1/console.log')
    copy(RAW/'vm-command.json','runs/attempt-1/command.json')
    for number in range(2,6):tree(RAW/f'attempt-{number}',f'runs/attempt-{number}')
    tree(RAW/'resolution','runs/dependency-resolution')
    copy(RAW/'resolve-command.json','runs/dependency-resolution/command.json')
    copy(RAW/'repository-build.log','inputs/repository-build.log')
    copy(RAW/'complete/repository-manifest.json','inputs/repository-manifest.json')
    verification=read(RAW/'repository-independent-verification.json')
    assert verification['archives_verified_against_signed_indices']==105
    copy(RAW/'repository-independent-verification.json','inputs/repository-independent-verification.json')
    tree(RAW/'verified-signed-indices','inputs/verified-signed-indices')
    for name in ['source.json','flatpak-1.16.6.tar.gz','upstream-tag.json','upstream-tag-object.json','pinned-source-verification.json']:
        copy(RAW/'inputs'/name,Path('inputs/upstream')/name)
    for name in ['common/flatpak-run.c','app/flatpak-builtins-enter.c','tests/make-test-runtime.sh']:
        copy(RAW/'inputs/upstream'/name,Path('inputs/upstream/source')/name)
    with tarfile.open(RAW/'inputs/flatpak-1.16.6.tar.gz') as archive:
        name=read(RAW/'inputs/source.json')['archive_prefix']+'/COPYING'
        (OUT/'inputs/upstream/COPYING').write_bytes(archive.extractfile(name).read())
    for name in ['host-preservation.json','product-unchanged-prepublication.json']:
        copy(RAW/name,Path('runs')/name)
    (OUT/'README.md').write_text('''# Revision 24 Flatpak boundary observations

Managed Flatpak integration remains unsupported. The final 40-check VM result
includes seven observations reproducing candidate-wrapper defects, not a passing
game-compatibility claim. See reports/docs/FLATPAK_SESSION_BOUNDARIES_2026-10-28.md.

The unchanged OMEN package and 132 product-source files retain revision 24
attribution. Failed runs and the earlier 37-check probe are retained separately.
Runtime fixtures use an isolated user-owned mock service/private PPD server; the
suite's existing installed-service checks have separate root-sandbox attribution.

inputs/upstream/flatpak-1.16.6.tar.gz is the unchanged official upstream source
archive. Its own copyright/license notices are retained inside it. The three
extracted reference files retain their original notices; upstream COPYING is
included alongside them. Tag/source hashes pin provenance, not a verified
upstream signature. Ubuntu packages were verified against signed distro indices.
The 135,546,880-byte offline APT repository remains in target/flatpak-scope-probe/
complete/complete-repository.tar with its full hash and 105 package entries in
inputs/repository-manifest.json; the large archive is not duplicated here.
''')
    files=[{'path':str(path.relative_to(OUT)),'bytes':path.stat().st_size,'sha256':digest(path)}
        for path in sorted(OUT.rglob('*')) if path.is_file()]
    summary={'result':'observations_verified','managed_flatpak_supported':False,
        'product_changed':False,'package_revision':24,'package':str(package.relative_to(ROOT)),
        'package_bytes':package.stat().st_size,'package_sha256':digest(package),
        'product_sources_unchanged':len(sources),'final_vm_checks':40,'flatpak_observation_checks':7,
        'physical_hardware_writes':False,'real_flatpak_game_tested':False,
        'fixture_profile_provider_private':True,'host_preservation':read(RAW/'host-preservation.json'),
        'prior_source_baseline':str(baseline.relative_to(ROOT)),'prior_source_manifest_sha256':digest(baseline),
        'upstream':read(RAW/'inputs/pinned-source-verification.json'),
        'offline_repository_sha256':final['offline_repository_sha256'],
        'files':files}
    write('validation-summary.json',summary)
    print(json.dumps({'evidence_files':len(files),'summary_sha256':digest(OUT/'validation-summary.json'),
        'product_sources':len(sources),'tests':len(tests),'documents':len(documents)},indent=2))

if __name__=='__main__':main()
