#!/usr/bin/env python3
"""Supplement unchanged revision 24 with actual native notification workflows."""
import datetime
import hashlib
import json
from pathlib import Path
import shutil

ROOT=Path(__file__).resolve().parents[1]
RAW=ROOT/'target/notification-workflow-v24'
OUT=ROOT/'docs/evidence/notification-workflow-v24'


def sha(path):return hashlib.file_digest(path.open('rb'),'sha256').hexdigest()
def read(path):return json.loads(path.read_text())
def copy(source,target):
    destination=OUT/target;destination.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(source,destination)


def main():
    assert not OUT.exists(),'published evidence is immutable'
    original=ROOT/'docs/evidence/sensor-browser-v24';baseline=read(original/'validation-summary.json')
    for entry in baseline['evidence_files']:assert sha(original/entry['path'])==entry['sha256'],entry['path']
    sources=read(original/'source-manifest.json')
    for entry in sources:assert sha(ROOT/entry['path'])==entry['sha256'],entry['path']
    package=ROOT/baseline['package'];assert sha(package)==baseline['package_sha256']
    script=ROOT/'tests/gui_notification_workflow.py';native={}
    for profile in ['debug','release']:
        result=read(RAW/(profile+'-final/notification-workflow.json'))
        assert result['result']=='passed' and result['cleanup_complete'] and len(result['checks_passed'])==7
        expected=baseline['binaries']['omen-gui'] if profile=='release' else baseline['native_browser']['debug']['gui_sha256']
        assert sha(ROOT/'target'/profile/'omen-gui')==result['binary_sha256']==expected
        assert result['test_script_sha256']==sha(script)
        messages=result['messages'];assert len(messages)==2 and all('MOCK' in message['body'] for message in messages)
        assert messages[1]['received_monotonic']-messages[0]['received_monotonic']>=30
        assert all('default' in message['actions'] for message in messages)
        native[profile]={'checks':7,'gui_sha256':expected,'notifications_received':2,
            'actual_message_interval_seconds':messages[1]['received_monotonic']-messages[0]['received_monotonic']}
    OUT.mkdir(parents=True);copy(Path(__file__),'collect-evidence.py')
    for path in sorted(RAW.rglob('*')):
        if path.is_file():copy(path,Path('runs')/path.relative_to(RAW))
    tests=[script,ROOT/'tests/native_desktop.py']
    test_manifest=[]
    for path in tests:
        test_manifest.append({'path':str(path.relative_to(ROOT)),'sha256':sha(path)})
        copy(path,Path('sources')/path.relative_to(ROOT))
    (OUT/'test-manifest.json').write_text(json.dumps(test_manifest,indent=2)+'\n')
    (OUT/'source-manifest.json').write_text(json.dumps(sources,indent=2)+'\n')
    reports=[ROOT/'docs'/name for name in ['NOTIFICATION_WORKFLOW_2026-10-28.md','APPLICATION_AUDIT_IMPLEMENTATION.md','IMPLEMENTATION_STATUS.md','TEST_REPORT.md','ACCESSIBILITY.md','CONTINUATION_CHECKLIST.md']]
    documentation=[]
    for path in reports:
        documentation.append({'path':str(path.relative_to(ROOT)),'sha256':sha(path)})
        copy(path,Path('reports')/path.relative_to(ROOT))
    (OUT/'documentation-manifest.json').write_text(json.dumps(documentation,indent=2)+'\n')
    evidence=[{'path':str(path.relative_to(OUT)),'sha256':sha(path),'bytes':path.stat().st_size} for path in sorted(OUT.rglob('*')) if path.is_file()]
    summary={'result':'passed','revision':'0.1.0-24','recorded_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'purpose':'Close direct checkbox/field/save/delivery/action/error-recovery coverage; no application or package change.',
        'package':str(package.relative_to(ROOT)),'package_sha256':sha(package),'package_bytes':package.stat().st_size,
        'product_source_files_verified':len(sources),'original_summary_sha256':sha(original/'validation-summary.json'),
        'original_evidence_files_rehashed':len(baseline['evidence_files']),'native_workflows':native,
        'test_script_sha256':sha(script),'actual_notification_action_signal':True,'real_cooldown_elapsed':True,
        'private_notification_server':True,'real_desktop_notified':False,'host_keyboard_events':False,'physical_hardware_writes':False,
        'host_packages_or_services_installed':False,'english_only':True,
        'limits':['XTEST uses a private authenticated Xwayland server; physical desktop input/placement is unverified.',
            'The receiver is a private D-Bus notification fixture; no physical desktop notification or compositor policy is claimed.',
            'Native UI coverage does not establish complete accessibility, audible synthesis, broader desktops or hardware validation.'],
        'evidence_files':evidence}
    (OUT/'validation-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps({key:value for key,value in summary.items() if key!='evidence_files'},indent=2))


if __name__=='__main__':main()
