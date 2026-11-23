#!/usr/bin/env python3
"""Publish exact sensor-browser evidence once; previous publications stay immutable."""
import datetime
import hashlib
import json
from pathlib import Path
import re
import shutil

ROOT=Path(__file__).resolve().parents[1]
RAW=ROOT/'target/sensor-browser-v24'
OUT=ROOT/'docs/evidence/sensor-browser-v24'


def sha(path):return hashlib.file_digest(path.open('rb'),'sha256').hexdigest()
def read(path):return json.loads(path.read_text())
def copy(source,target):
    destination=OUT/target;destination.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(source,destination)
def manifest(paths):return [{'path':str(path.relative_to(ROOT)),'sha256':sha(path)} for path in sorted(set(paths)) if path.is_file()]


def main():
    assert not OUT.exists(),'published evidence is immutable'
    package=ROOT/'target/debian/omen-linux_0.1.0-24_amd64.deb'
    vm=read(RAW/'fresh-vm/vm-systemd.json');static=read(RAW/'final-static/package-validation.json')
    assert vm['result']=='passed' and len(vm['checks_passed'])==38
    assert vm['vm_exited'] and vm['vm_exit_code']==0 and vm['temporary_files_removed']
    helper=read(RAW/'current-helper.json');assert helper['selected_from_current_cargo_artifact'] and sha(Path(helper['path']))==helper['sha256']
    assert vm['managed_session_fixture']['helper_sha256']==helper['sha256'] and vm['managed_session_fixture']['cleanup_complete']
    assert vm['source_switch']['result']=='passed' and len(vm['source_switch']['checks_passed'])==5
    assert sha(package)==static['sha256'] and static['vm_equivalence']['reference_sha256']==vm['package_sha256']
    assert static['vm_equivalence']['runtime_payload_identical'] and static['vm_equivalence']['maintainer_scripts_and_control_identical']
    gui=sha(ROOT/'target/release/omen-gui');rust={};native={}
    for profile in ['debug','release']:
        log=(RAW/(profile+'-rust-final.log')).read_text();rust[profile]=sum(map(int,re.findall(r'test result: ok\. (\d+) passed',log)))
        assert rust[profile]==276 and 'test result: FAILED' not in log
        result=read(RAW/(profile+'-native-final/sensor-browser.json'))
        assert result['result']=='passed' and len(result['checks_passed'])==9 and result['cleanup_complete']
        assert result['gui_sha256']==sha(ROOT/'target'/profile/'omen-gui')
        native[profile]={'checks':9,'thousand_readings':result['thousand_reading_observation'],'gui_sha256':result['gui_sha256']}
    for backend in ['x11','wayland']:
        result=read(RAW/('native-'+backend+'-final/gui-settings-release.json'))
        assert result['result']=='passed' and result['binary_sha256']==gui and result['cleanup_complete']
        assert sum(len(result[key]) for key in ['checks_passed','network_checks_passed','health_checks_passed'])==31
    keyboard=read(RAW/'keyboard-final/keyboard-orca.json')
    assert keyboard['result']=='passed' and len(keyboard['checks_passed'])==13 and keyboard['cleanup_complete'] and keyboard['gui_sha256']==gui
    scene=read(RAW/'presentation-confirmation/sensor-presentation.json')
    assert scene['result']=='passed' and scene['gui_sha256']==gui and len(scene['cases'])==5 and scene['cleanup_complete']
    assert sum(len(case['contrast']) for case in scene['cases'])==190
    assert 'Finished' in (RAW/'clippy-final.log').read_text() and not (RAW/'fmt-final.log').read_text().strip()
    preserved={}
    for name in ['source-switch-v23','source-switch-v23-current-helper','control-recovery-v22','desktop-v21','real-game-v21']:
        base=ROOT/'docs/evidence'/name;summary=read(base/'validation-summary.json')
        for entry in summary['evidence_files']:assert sha(base/entry['path'])==entry['sha256'],(name,entry['path'])
        preserved[name]={'summary_sha256':sha(base/'validation-summary.json'),'evidence_files_rehashed':len(summary['evidence_files'])}
    baseline=read(ROOT/'docs/evidence/source-switch-v23/source-manifest.json')
    changes=[entry['path'] for entry in baseline if sha(ROOT/entry['path'])!=entry['sha256']]
    assert all(path.startswith('gui/') or path in ['packaging/build-deb.sh','packaging/debian/changelog'] for path in changes),changes
    old=read(ROOT/'docs/evidence/source-switch-v23/validation-summary.json')['binaries']
    binaries={name:sha(ROOT/'target/release'/name) for name in ['omen-cli','omen-daemon','omen-gui']}
    assert all(binaries[name]==old[name] for name in ['omen-cli','omen-daemon'])
    OUT.mkdir(parents=True);copy(Path(__file__),'collect-evidence.py')
    for path in sorted(RAW.rglob('*')):
        if path.is_file() and 'candidate' not in path.relative_to(RAW).parts:copy(path,Path('runs')/path.relative_to(RAW))
    sources=[]
    for directory in ['crates','gui','packaging']:sources.extend(path for path in (ROOT/directory).rglob('*') if path.is_file() and 'target' not in path.relative_to(ROOT).parts)
    sources.extend([ROOT/'Cargo.toml',ROOT/'Cargo.lock'])
    tests=[ROOT/'tests'/name for name in ['gui_sensor_browser.py','gui_sensor_presentation.py','gui_accessibility.py','gui_settings.py',
        'gui_source_switch.py','gui_source_presentation.py','source_switch_vm_inner.py','gui_control_recovery.py','native_desktop.py',
        'gui_presentation.py','systemd_vm.py','systemd_vm_inner.py','graphical_polkit_vm_inner.py','managed_games_vm_inner.py',
        'coarse_recovery_vm_inner.py','nvidia_reader_vm_inner.py','package.py']]
    reports=[ROOT/'README.md',*[ROOT/'docs'/name for name in ['SENSOR_BROWSER_2026-10-28.md','APPLICATION_AUDIT_IMPLEMENTATION.md',
        'IMPLEMENTATION_STATUS.md','CONTINUATION_CHECKLIST.md','INSTALLATION.md','ACCESSIBILITY.md','TEST_REPORT.md','PACKAGE_VALIDATION.md','VM_VALIDATION.md']]]
    for label,paths in [('source',sources),('test',tests),('documentation',reports)]:
        (OUT/(label+'-manifest.json')).write_text(json.dumps(manifest(paths),indent=2)+'\n')
        for path in paths:copy(path,Path('reports' if label=='documentation' else 'sources')/path.relative_to(ROOT))
    evidence=[{'path':str(path.relative_to(OUT)),'sha256':sha(path),'bytes':path.stat().st_size} for path in sorted(OUT.rglob('*')) if path.is_file()]
    summary={'result':'passed','revision':'0.1.0-24','recorded_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'package':str(package.relative_to(ROOT)),'package_sha256':sha(package),'package_bytes':package.stat().st_size,
        'candidate_sha256':vm['package_sha256'],'runtime_control_equivalence':static['vm_equivalence'],'binaries':binaries,
        'rust_tests':rust,'native_browser':native,'native_regressions_per_backend':31,'keyboard_orca_workflows':13,
        'presentation_scenes':5,'active_text_contrast_samples':190,'fresh_vm_checks':38,'installed_source_checks':5,'guest_reboots':2,
        'current_native_helper':helper,'cli_and_daemon_unchanged_from_revision23':True,'strict_clippy':True,'formatting':True,
        'visual_review':'One initial batch exposed long selected text overflow; one confirmation batch of five scenes passed. No subsequent UI edit.',
        'preserved_historical_evidence':preserved,'changed_baseline_sources':changes,
        'host_installation':False,'physical_hardware_writes':False,'english_only':True,
        'retained_failures':['Initial focus labels exposed selected values without field context.',
            'Device search needed an explicit string expression with the custom item factory; earlier keyboard harness paths did not select a device.',
            'Initial long selected picker text overflowed its allocation; confirmation uses ellipsis with full native text/tooltip.',
            'Initial empty-state developer capture hit a transient missing node; bounded retry replaces panic.'],
        'limits':['Private XTEST and generated Orca speech do not establish physical input/DPI, audible synthesis or complete accessibility.',
            '1000-reading latency/memory are single fixture observations; larger datasets and other devices remain unmeasured.',
            'No new revision 24 real KDE/PAM or real-store-game result is claimed. Earlier revision 23/21 evidence retains its attribution.',
            'Physical hardware, suspend/resume, broader distributions and broader launcher/runtime coverage remain full-audit gates.'],
        'evidence_files':evidence}
    (OUT/'validation-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps({key:value for key,value in summary.items() if key!='evidence_files'},indent=2))


if __name__=='__main__':main()
