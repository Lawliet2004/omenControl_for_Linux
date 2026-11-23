#!/usr/bin/env python3
"""Publish revision 23 source-recovery evidence once, preserving older releases."""
import datetime
import hashlib
import json
from pathlib import Path
import re
import shutil

ROOT=Path(__file__).resolve().parents[1]
RAW=ROOT/'target/source-switch-v23'
OUT=ROOT/'docs/evidence/source-switch-v23'


def sha(path): return hashlib.file_digest(path.open('rb'),'sha256').hexdigest()
def read(path): return json.loads(path.read_text())
def copy(source,target):
    destination=OUT/target; destination.parent.mkdir(parents=True,exist_ok=True); shutil.copyfile(source,destination)
def manifest(paths): return [{'path':str(path.relative_to(ROOT)),'sha256':sha(path)} for path in sorted(set(paths)) if path.is_file()]


def main():
    assert not OUT.exists(),'published evidence is immutable'
    package=ROOT/'target/debian/omen-linux_0.1.0-23_amd64.deb'
    vm=read(RAW/'fresh-vm-confirmed/vm-systemd.json')
    earlier=read(RAW/'fresh-vm/vm-systemd.json')
    authorization=earlier['partial_result']['graphical_authorization']
    static=read(RAW/'final-static/package-validation.json')
    assert vm['result']=='passed' and len(vm['checks_passed'])==38
    assert earlier['package_sha256']==vm['package_sha256'] and earlier['vm_exited'] and earlier['temporary_files_removed']
    assert vm['vm_exited'] and vm['vm_exit_code']==0 and vm['temporary_files_removed']
    assert vm['source_switch']['result']=='passed' and len(vm['source_switch']['checks_passed'])==5
    assert vm['source_switch']['fixture_cleanup_complete']
    assert authorization['result']=='passed' and len(authorization['checks_passed'])==6 and authorization['fixture_cleanup_complete']
    assert sha(package)==static['sha256']
    assert static['vm_equivalence']['reference_sha256']==vm['package_sha256']
    assert static['vm_equivalence']['runtime_payload_identical'] and static['vm_equivalence']['maintainer_scripts_and_control_identical']
    binaries={name:sha(ROOT/'target/release'/name) for name in ['omen-gui','omen-cli','omen-daemon']}
    rust={}
    for profile in ['debug','release']:
        log=(RAW/(profile+'-rust'+('-final' if profile=='debug' else '')+'.log')).read_text()
        rust[profile]=sum(map(int,re.findall(r'test result: ok\. (\d+) passed',log)))
        assert rust[profile]==271 and 'test result: FAILED' not in log
        for folder,file,count in [(profile+'-native'+('-final' if profile=='debug' else ''),'source-switch.json',6),(profile+'-controls-final','control-recovery.json',12)]:
            result=read(RAW/folder/file)
            assert result['result']=='passed' and result['cleanup_complete'] and len(result['checks_passed'])==count
            assert result['gui_sha256']==sha(ROOT/'target'/profile/'omen-gui')
    for backend in ['x11','wayland']:
        result=read(RAW/('native-'+backend+'-final/gui-settings-release.json'))
        assert result['result']=='passed' and result['cleanup_complete'] and result['binary_sha256']==binaries['omen-gui']
        assert sum(len(result[key]) for key in ['checks_passed','network_checks_passed','health_checks_passed'])==31
    managed=read(RAW/'managed-release/managed-games-native.json')
    assert managed['result']=='passed' and managed['cleanup_complete'] and len(managed['checks_passed'])==20
    scene=read(RAW/'presentation/source-presentation.json')
    assert scene['result']=='passed' and scene['cleanup_complete'] and scene['gui_sha256']==binaries['omen-gui'] and len(scene['cases'])==6
    assert sum(len(case['contrast']) for case in scene['cases'])==185
    assert 'Finished' in (RAW/'clippy-final.log').read_text() and not (RAW/'fmt-final.log').read_text().strip()
    preservation=read(RAW/'historical-preservation.json')
    for name,record in preservation.items():
        base=ROOT/'docs/evidence'/name; assert sha(base/'validation-summary.json')==record['summary_sha256']
        for item in read(base/'validation-summary.json')['evidence_files']: assert sha(base/item['path'])==item['sha256']
    OUT.mkdir(parents=True)
    copy(Path(__file__),'collect-evidence.py')
    for path in sorted(RAW.rglob('*')):
        if not path.is_file() or any(part in ['candidate','final-package'] for part in path.relative_to(RAW).parts): continue
        copy(path,Path('runs')/path.relative_to(RAW))
    sources=[]
    for directory in ['crates','gui','packaging']:
        sources.extend(path for path in (ROOT/directory).rglob('*') if path.is_file() and 'target' not in path.relative_to(ROOT).parts)
    sources.extend([ROOT/'Cargo.toml',ROOT/'Cargo.lock'])
    tests=[ROOT/'tests'/name for name in ['gui_source_switch.py','gui_source_presentation.py','source_switch_vm_inner.py',
        'gui_control_recovery.py','native_desktop.py','gui_presentation.py','gui_settings.py','systemd_vm.py','systemd_vm_inner.py',
        'graphical_polkit_vm_inner.py','managed_games_vm_inner.py','coarse_recovery_vm_inner.py','nvidia_reader_vm_inner.py','package.py']]
    reports=[ROOT/'README.md',*[ROOT/'docs'/name for name in ['SOURCE_SWITCHING_2026-10-28.md','APPLICATION_AUDIT_IMPLEMENTATION.md',
        'IMPLEMENTATION_STATUS.md','CONTINUATION_CHECKLIST.md','INSTALLATION.md','TEST_REPORT.md','ACCESSIBILITY.md','PACKAGE_VALIDATION.md','VM_VALIDATION.md']]]
    for label,paths in [('source',sources),('test',tests),('documentation',reports)]:
        entries=manifest(paths)
        (OUT/(label+'-manifest.json')).write_text(json.dumps(entries,indent=2)+'\n')
        for path in paths: copy(path,Path('sources' if label!='documentation' else 'reports')/path.relative_to(ROOT))
    evidence=[{'path':str(path.relative_to(OUT)),'sha256':sha(path),'bytes':path.stat().st_size} for path in sorted(OUT.rglob('*')) if path.is_file()]
    summary={'result':'passed','revision':'0.1.0-23','recorded_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'package':str(package.relative_to(ROOT)),'package_sha256':sha(package),'package_bytes':package.stat().st_size,
        'candidate_package_sha256':vm['package_sha256'],'runtime_control_equivalence':static['vm_equivalence'],
        'binaries':binaries,'rust_tests_per_profile':rust,'strict_clippy':True,'formatting':True,
        'native_source_workflows_per_profile':6,'native_control_keyboard_workflows_per_profile':12,
        'native_settings_health_network_per_backend':31,'private_managed_game_checks':20,
        'fresh_vm_checks':38,'matching_candidate_prior_pam_checks':6,'combined_distinct_vm_checks':44,'installed_default_source_checks':5,'real_kde_pam_checks':6,'guest_reboots':2,
        'main_window_scenes':6,'companion_overlay_captures':6,'active_text_contrast_samples':185,
        'visual_review':'One batched source-state review; VM source captures confirmed the installed desktop. No subsequent UI edit.',
        'historical_preservation':preservation,'host_packages_or_services_installed':False,'physical_hardware_writes':False,'english_only':True,
        'retained_harness_failures':['Old total-outage regression expected stale timer; split endpoint loss and blocked acquisition coverage.',
            'Old disconnected-preview regression expected only a connection error; current unavailable-monitoring guard can block before sending.',
            'Initial source observer encountered a defunct accessibility object during desktop startup; retry handling was corrected. Its six KDE/PAM checks passed on the identical candidate before that harness failure.',
            'Second source observer checked the generic About copy for a fixture model; the assertion now checks the actual Model field.',
            'Third source observer asserted availability before the restart update completed; it now waits for the new usable report before requesting diagnostics.'],
        'limits':['Physical hardware, input/DPI, suspend/resume, complete accessibility and broader distributions remain unvalidated.',
            'Revision 21 Lutris/OpenTTD evidence remains separate; revision 23 is not newly tested with a real store game.',
            'Root-service trust verifies a local endpoint and reported identity, not hardware-validation semantics or compromised root.'],
        'evidence_files':evidence}
    (OUT/'validation-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps({key:value for key,value in summary.items() if key!='evidence_files'},indent=2))


if __name__=='__main__': main()
