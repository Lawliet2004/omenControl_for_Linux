#!/usr/bin/env python3
"""Publish revision 22's exact control recovery evidence once."""
import datetime
import hashlib
import json
from pathlib import Path
import re
import shutil

ROOT = Path(__file__).resolve().parents[1]
RAW = ROOT / 'target/control-recovery-v22'
OUT = ROOT / 'docs/evidence/control-recovery-v22'


def sha(path):
    return hashlib.file_digest(path.open('rb'), 'sha256').hexdigest()


def copy(source, target):
    dest = OUT / target
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, dest)


def manifest(paths):
    return [{'path':str(p.relative_to(ROOT)), 'sha256':sha(p)} for p in sorted(set(paths)) if p.is_file()]


def main():
    assert not OUT.exists(), 'published evidence is immutable'
    vm = json.loads((RAW/'fresh-vm-final/vm-systemd.json').read_text())
    static = json.loads((RAW/'final-static/package-validation.json').read_text())
    package = ROOT/'target/debian/omen-linux_0.1.0-22_amd64.deb'
    assert vm['result']=='passed' and len(vm['checks_passed'])==39
    assert vm['vm_exit_code']==0 and vm['temporary_files_removed']
    assert vm['package_sha256']==static['vm_equivalence']['reference_sha256']
    assert static['vm_equivalence']['runtime_payload_identical'] and static['vm_equivalence']['maintainer_scripts_and_control_identical']
    assert sha(package)==static['sha256']
    gui_sha=sha(ROOT/'target/release/omen-gui')
    native={}
    for profile in ['debug','release']:
        item=json.loads((RAW/(profile+'-native-boundaries/control-recovery.json')).read_text())
        assert item['result']=='passed' and len(item['checks_passed'])==12 and item['cleanup_complete']
        assert item['gui_sha256']==sha(ROOT/'target'/profile/'omen-gui')
        native[profile]={'checks':12,'gui_sha256':item['gui_sha256'],'source_sha256':item['test_script_sha256']}
        assert sha(RAW/(profile+'-native-boundaries/test-source.py'))==item['test_script_sha256']
    for backend in ['x11','wayland']:
        item=json.loads((RAW/('native-'+backend+'-final/gui-settings-release.json')).read_text())
        assert item['result']=='passed' and item['binary_sha256']==gui_sha and item['cleanup_complete']
        assert sum(len(item[k]) for k in ['checks_passed','network_checks_passed','health_checks_passed'])==31
    presentation=json.loads((RAW/'presentation-confirmation/controls-presentation.json').read_text())
    assert presentation['result']=='passed' and presentation['gui_sha256']==gui_sha and len(presentation['cases'])==12
    assert sum(len(c['contrast']) for c in presentation['cases'])==273
    rust={}
    for profile in ['debug','release']:
        log=(RAW/(profile+'-rust-final.log')).read_text()
        rust[profile]=sum(map(int,re.findall(r'test result: ok\. (\d+) passed',log)))
        assert rust[profile]==264 and 'test result: FAILED' not in log
    assert 'Finished' in (RAW/'clippy-final-range.log').read_text()
    assert not (RAW/'fmt-final.log').read_text().strip()
    OUT.mkdir(parents=True)
    copy(Path(__file__),'collect-evidence.py')
    folders=['fresh-vm-final','fresh-vm','final-static','documentation-draft-static',
        'debug-native-boundaries','release-native-boundaries','native-x11-final','native-wayland-final',
        'presentation-confirmation','presentation','negative-v21','negative-v21-final','negative-range',
        'debug-native','debug-native-retry','debug-native-final','release-native','release-keyboard',
        'release-keyboard-final','debug-keyboard','native-x11','native-wayland']
    for folder in folders:
        for p in sorted((RAW/folder).rglob('*')):
            if p.is_file(): copy(p,Path('runs')/folder/p.relative_to(RAW/folder))
    for p in sorted(RAW.glob('*')):
        if p.is_file() and p.suffix in ['.log','.json']:copy(p,Path('runs')/p.name)
    copy(RAW/'pre-range/control_pages.rs','history/control_pages_before_range_fix.rs')
    source_paths=[]
    for directory in ['crates','gui','packaging']:
        source_paths += [p for p in (ROOT/directory).rglob('*') if p.is_file() and 'target' not in p.relative_to(ROOT).parts]
    source_paths += [ROOT/'Cargo.toml',ROOT/'Cargo.lock']
    sources=manifest(source_paths)
    for entry in sources:copy(ROOT/entry['path'],Path('sources')/entry['path'])
    tests=[ROOT/'tests'/name for name in ['gui_control_recovery.py','gui_controls_presentation.py','native_desktop.py',
        'gui_presentation.py','gui_settings.py','systemd_vm.py','systemd_vm_inner.py','graphical_polkit_vm_inner.py',
        'managed_games_vm_inner.py','coarse_recovery_vm_inner.py','nvidia_reader_vm_inner.py','package.py']]
    for p in tests:copy(p,Path('sources')/p.relative_to(ROOT))
    reports=[ROOT/'README.md',*[ROOT/'docs'/name for name in ['CONTROL_PAGE_RECOVERY_2026-10-28.md',
        'APPLICATION_AUDIT_IMPLEMENTATION.md','IMPLEMENTATION_STATUS.md','TEST_REPORT.md','ACCESSIBILITY.md',
        'INSTALLATION.md','PACKAGE_VALIDATION.md','VM_VALIDATION.md','CONTINUATION_CHECKLIST.md']]]
    for p in reports:copy(p,Path('reports')/p.relative_to(ROOT))
    for name,entries in [('source-manifest.json',sources),('test-manifest.json',manifest(tests)),('documentation-manifest.json',manifest(reports))]:
        (OUT/name).write_text(json.dumps(entries,indent=2)+'\n')
    preserved={}
    for name in ['desktop-v21','real-game-v21','deployment-v20','launcher-expansion']:
        base=ROOT/'docs/evidence'/name
        old=json.loads((base/'validation-summary.json').read_text())
        for entry in old['evidence_files']:assert sha(base/entry['path'])==entry['sha256'],(name,entry['path'])
        preserved[name]={'summary_sha256':sha(base/'validation-summary.json'),'evidence_files_rehashed':len(old['evidence_files'])}
    baseline=json.loads((ROOT/'docs/evidence/desktop-v21/source-manifest.json').read_text())
    changed=[e['path'] for e in baseline if (ROOT/e['path']).is_file() and sha(ROOT/e['path'])!=e['sha256']]
    assert all(p.startswith('gui/') or p in ['packaging/debian/changelog','packaging/build-deb.sh'] for p in changed),changed
    evidence=[{'path':str(p.relative_to(OUT)),'sha256':sha(p),'bytes':p.stat().st_size} for p in sorted(OUT.rglob('*')) if p.is_file()]
    summary={'result':'passed','recorded_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'revision':'0.1.0-22','package':str(package.relative_to(ROOT)),'package_sha256':sha(package),'package_bytes':package.stat().st_size,
        'candidate_sha256':vm['package_sha256'],'gui_sha256':gui_sha,'runtime_control_equivalence':static['vm_equivalence'],
        'rust_tests':rust,'strict_clippy':True,'fmt':True,'native_recovery_keyboard':native,
        'native_regressions_per_backend':31,'fresh_final_vm_checks':39,'real_graphical_authorization_checks':6,
        'presentation_scenes':12,'active_text_contrast_samples':273,
        'visual_review':'One batch of 12 initial scenes; one batch of four fan confirmation scenes after the range fix; no subsequent UI edit.',
        'cli_daemon_unchanged_from_revision21':True,'changed_baseline_sources':changed,
        'preserved_historical_evidence':preserved,'host_packages_or_services_installed':False,'physical_hardware_writes':False,
        'english_only':True,'limits':['Recovery follows the selected source; direct-to-daemon migration and service/source provenance refresh remain open.',
            'Private XTEST/AT-SPI and fixture writes; physical focus restoration, complete accessibility and hardware controls are unvalidated.',
            'Revision 21 real-game results remain separately attributed; the revision 22 GUI was not retested with a store game.'],
        'evidence_files':evidence}
    (OUT/'validation-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps({k:v for k,v in summary.items() if k not in ['evidence_files','runtime_control_equivalence']},indent=2))


if __name__=='__main__':main()
