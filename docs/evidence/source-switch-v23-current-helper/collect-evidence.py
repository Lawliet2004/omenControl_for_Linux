#!/usr/bin/env python3
"""Supplement immutable revision 23 evidence with the current session helper."""
import datetime
import hashlib
import json
from pathlib import Path
import shutil

ROOT=Path(__file__).resolve().parents[1]
RAW=ROOT/'target/source-switch-v23-current-helper'
OUT=ROOT/'docs/evidence/source-switch-v23-current-helper'
EXPECTED='f5e322fdb994d9e9bc0f209b7c67c8159a4f33560f52ba659a3fb6a73013a6ad'


def sha(path):return hashlib.file_digest(path.open('rb'),'sha256').hexdigest()
def read(path):return json.loads(path.read_text())
def copy(source,target):
    destination=OUT/target; destination.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(source,destination)


def main():
    assert not OUT.exists(),'published evidence is immutable'
    prior=ROOT/'docs/evidence/source-switch-v23';base=read(prior/'validation-summary.json')
    for entry in base['evidence_files']:assert sha(prior/entry['path'])==entry['sha256']
    package=ROOT/base['package'];assert sha(package)==base['package_sha256']
    helper=ROOT/'target/release/deps/managed_games_native-4cff964311c5e5ac';assert sha(helper)==EXPECTED
    vm=read(RAW/'fresh-vm/vm-systemd.json')
    assert vm['result']=='passed' and len(vm['checks_passed'])==38
    assert vm['vm_exited'] and vm['vm_exit_code']==0 and vm['temporary_files_removed']
    assert vm['package_sha256']==sha(package)
    assert vm['managed_session_fixture']['helper_sha256']==EXPECTED
    assert vm['managed_session_fixture']['cleanup_complete'] and vm['managed_session_fixture']['production_polkit']
    assert vm['source_switch']['result']=='passed' and len(vm['source_switch']['checks_passed'])==5
    assert vm['source_switch']['fixture_cleanup_complete']
    native=read(RAW/'managed-release/managed-games-native.json')
    assert native['result']=='passed' and native['cleanup_complete'] and len(native['checks_passed'])==20
    # There has been no app/runtime change since the first publication.
    sources=read(prior/'source-manifest.json')
    for entry in sources:assert sha(ROOT/entry['path'])==entry['sha256'],entry['path']
    tests=read(prior/'test-manifest.json')
    for entry in tests:assert sha(ROOT/entry['path'])==entry['sha256'],entry['path']
    OUT.mkdir(parents=True)
    copy(Path(__file__),'collect-evidence.py')
    for path in sorted(RAW.rglob('*')):
        if path.is_file():copy(path,Path('runs')/path.relative_to(RAW))
    for label,data in [('source',sources),('test',tests)]:
        (OUT/(label+'-manifest.json')).write_text(json.dumps(data,indent=2)+'\n')
    reports=[ROOT/'docs'/name for name in ['SOURCE_SWITCHING_2026-10-28.md','APPLICATION_AUDIT_IMPLEMENTATION.md','IMPLEMENTATION_STATUS.md',
        'TEST_REPORT.md','PACKAGE_VALIDATION.md','VM_VALIDATION.md']]
    docs=[]
    for path in reports:
        docs.append({'path':str(path.relative_to(ROOT)),'sha256':sha(path)})
        copy(path,Path('reports')/path.relative_to(ROOT))
    (OUT/'documentation-manifest.json').write_text(json.dumps(docs,indent=2)+'\n')
    evidence=[{'path':str(path.relative_to(OUT)),'sha256':sha(path),'bytes':path.stat().st_size} for path in sorted(OUT.rglob('*')) if path.is_file()]
    summary={'result':'passed','revision':'0.1.0-23','recorded_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'purpose':'Correct native managed-session helper attribution; no app or final package change.',
        'supersedes_helper_attribution_in':'source-switch-v23/validation-summary.json','prior_summary_sha256':sha(prior/'validation-summary.json'),
        'prior_evidence_files_rehashed':len(base['evidence_files']),
        'package':str(package.relative_to(ROOT)),'package_sha256':sha(package),'package_bytes':package.stat().st_size,
        'current_helper':str(helper.relative_to(ROOT)),'current_helper_sha256':EXPECTED,
        'cached_helper_sha256':'1af6ca2cc16173953d24cea78168f889896427c3bb639075c59ba2dc796508fa',
        'current_helper_native_checks':20,'fresh_final_package_vm_checks':38,'installed_current_helper_checks':3,
        'installed_source_checks':5,'guest_reboots':2,'all_private_resources_removed':True,
        'product_and_test_sources_match_initial_publication':True,'host_installation':False,'physical_hardware_writes':False,
        'limits':base['limits'],'evidence_files':evidence}
    (OUT/'validation-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps({key:value for key,value in summary.items() if key!='evidence_files'},indent=2))


if __name__=='__main__':main()
