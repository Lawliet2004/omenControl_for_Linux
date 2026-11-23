from pathlib import Path
import datetime,gzip,hashlib,json,re,shutil

ROOT=Path.cwd();BASE=ROOT/'target/a11y-final';OUT=ROOT/'docs/evidence/accessibility'
OUT.mkdir(exist_ok=True)

def sha(path):return hashlib.file_digest(path.open('rb'),'sha256').hexdigest()
def read(path):return json.loads(path.read_text())
def copy(source,destination):
 destination.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(source,destination)
def manifest(paths,name):
 entries=[{'path':str(p.relative_to(ROOT)),'sha256':sha(p)} for p in sorted(set(paths)) if p.is_file()]
 value={'hash_method':'SHA-256 of sorted sha256 + two spaces + relative path + newline records','files':entries}
 value['sha256']=hashlib.sha256(''.join(e['sha256']+'  '+e['path']+'\n' for e in entries).encode()).hexdigest()
 (OUT/name).write_text(json.dumps(value,indent=2)+'\n');return value['sha256']

runner=read(BASE/'runner.json');assert runner['result']=='passed' and len(runner['jobs'])==12
assert all(job['exit_code']==0 for job in runner['jobs'])
package=read(BASE/'package-static/package-validation.json')
assert package['version']=='0.1.0-19' and all(package['checks'].values())
assert sha(ROOT/package['package'])==package['sha256']
for name,data in package['binaries'].items():assert data['matches_release'] and sha(ROOT/'target/release'/name)==data['sha256']
gui_hash=package['binaries']['omen-gui']['sha256'];assert gui_hash==runner['gui_sha256']
copy(BASE/'runner.json',OUT/'runner.json');copy(ROOT/'target/run-a11y-release.py',OUT/'run-release-validation.py')
copy(BASE/'runtime-versions.json',OUT/'runtime-versions.json')
copy(ROOT/'target/collect-accessibility-evidence.py',OUT/'collect-evidence.py')
for name in ['package-build.log','package-static.log']:copy(BASE/name,OUT/name)
for p in (BASE/'package-static').iterdir():
 if p.is_file():copy(p,OUT/p.name)
for src,name in [('a11y-format.log','cargo-fmt.log'),('a11y-clippy-all.log','cargo-clippy.log'),
                 ('a11y-tests-debug.log','cargo-test-debug.log'),('a11y-tests-release.log','cargo-test-release.log')]:
 copy(ROOT/'target'/src,OUT/name)
rust={}
for profile in ['debug','release']:
 counts=[tuple(map(int,x)) for x in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', (OUT/f'cargo-test-{profile}.log').read_text())]
 assert sum(c[0] for c in counts)==253 and all(c[1]==c[2]==0 for c in counts)
 rust[profile]={'passed':253,'failed':0,'ignored':0}
keyboard={}
for profile,path in [('debug',ROOT/'target/a11y-debug/keyboard-final'),('release',BASE/'keyboard')]:
 data=read(path/'keyboard-orca.json');assert data['result']=='passed' and data['cleanup_complete'] and len(data['checks_passed'])==13
 assert data['gui_sha256']==sha(ROOT/'target'/profile/'omen-gui')
 assert not data['host_keyboard_events'] and not data['physical_hardware_writes']
 for expected in ['Pages','Temperature history','History readings','Search sensors','Search games','Reading held for selection','Desktop launch issues cleared','Notification preferences saved']:
  assert any(expected in line for line in data['speech_output']),expected
 copy(path/'keyboard-orca.json',OUT/f'keyboard-{profile}.json')
 copy(path/'gui.log',OUT/f'keyboard-{profile}-gui.log')
 (OUT/f'orca-{profile}.log.gz').write_bytes(gzip.compress((path/'orca-debug.log').read_bytes(),mtime=0))
 keyboard[profile]={'workflows':13,'evidence':f'keyboard-{profile}.json','generated_speech_utterances':len(data['speech_output']),
                    'private_XTEST_input':True,'host_keyboard_events':False,'audible_synthesis_tested':False}

matrix=read(BASE/'presentation/presentation.json');selected=read(BASE/'network-selection/network-selection.json')
for data in [matrix,selected]:assert data['result']=='passed' and data['cleanup_complete'] and data['gui_sha256']==gui_hash
assert len(matrix['cases'])==36 and len(selected['cases'])==2
for case in matrix['cases']+selected['cases']:
 assert all(not case[key] for key in ['horizontal_clipping','collapsed_text_allocations','contrast_failures'])
assert sum(len(c['contrast']) for c in matrix['cases'])==1041
assert sum(len(c['contrast']) for c in selected['cases'])==59
captures=[]
for group,data,record in [('presentation',matrix,'presentation.json'),('network-selection',selected,'network-selection.json')]:
 copy(BASE/group/record,OUT/group/record)
 for case in data['cases']:
  image=BASE/group/case['capture'];assert sha(image)==case['capture_sha256']
  destination=ROOT/'docs/screenshots/accessibility'/group/image.name;copy(image,destination)
  report=BASE/group/image.with_suffix('.json').name;copy(report,OUT/group/report.name)
  captures.append({'path':str(destination.relative_to(ROOT)),'sha256':sha(destination),'case':case['case'],
                   'report':str((OUT/group/report.name).relative_to(ROOT))})
 copy(BASE/(group+'.log'),OUT/(group+'.log'))
ui={};native_captures=[]
for backend in ['x11','wayland']:
 records={}
 for category,file in [('settings','gui-settings-release.json'),('library','library-ui.json'),('managed','gui-game-sessions-release.json'),('issues','gui-launch-issues-release.json'),('shortcuts',f'shortcuts-release-{backend}.json')]:
  path=BASE/f'{category}-{backend}';data=read(path/file)
  assert data['result']=='passed' and data['cleanup_complete']
  assert data.get('gui_sha256',data.get('binary_sha256'))==gui_hash,(category,backend)
  copy(path/file,OUT/f'{category}-{backend}.json');copy(BASE/f'{category}-{backend}.log',OUT/f'{category}-{backend}.log')
  for p in path.glob('*.log'):
   if p.is_file():copy(p,OUT/f'{category}-{backend}-{p.name}')
  for capture in data.get('captures',[]):
   source=ROOT/capture['path'];assert sha(source)==capture['sha256']
   destination=OUT/'native-captures'/category/backend/source.name;copy(source,destination)
   native_captures.append({'path':str(destination.relative_to(ROOT)),'sha256':sha(destination),'backend':backend,'category':category,'visually_reviewed':False})
  records[category]=data
 s,l,m,i,k=[records[c] for c in ['settings','library','managed','issues','shortcuts']]
 assert sum(len(s[key]) for key in ['checks_passed','network_checks_passed','health_checks_passed'])==31
 assert l['count']==10000 and len(l['checks_passed'])==5
 assert len(m['checks_passed'])-bool(m['captures'])==5 and len(i['checks_passed'])-bool(i['captures'])==6 and len(k['checks_passed'])==32
 assert m['cli_sha256']==i['cli_sha256']==package['binaries']['omen-cli']['sha256']
 ui[backend]={'native_regressions':31,'library_workflows':5,'library_entries':10000,'managed_workflows':5,
              'launch_recovery_workflows':6,'shortcut_overlay_workflows':32,'gtk_display_type':s['gtk_display_type'],
              'scan_and_first_accessible_response_seconds':l['scan_and_first_accessible_response_seconds'],
              'loaded_rss_kib':l['loaded_rss_kib'],'peak_observed_rss_kib':l['peak_observed_rss_kib'],
              'additional_capture_checks':int(bool(m['captures']))+int(bool(i['captures']))}

# Preserve the observed failure and the successful isolated check, with their
# earlier GUI identity. They do not substitute for final-binary results above.
for source,name in [('target/a11y-release/library-x11/library-ui.json','initial-library-timeout.json'),
                    ('target/a11y-release/library-x11-isolated/library-ui.json','initial-library-isolated.json'),
                    ('target/a11y-release/keyboard-network-before/keyboard-orca.json','initial-network-selection-loss.json')]:
 copy(ROOT/source,OUT/name)
initial=read(OUT/'initial-library-timeout.json');isolated=read(OUT/'initial-library-isolated.json')
assert initial['result']=='failed' and isolated['result']=='passed' and initial['gui_sha256']==isolated['gui_sha256']
previous=read(ROOT/'docs/evidence/launch-issues/validation-summary.json')
for name in ['omen-cli','omen-daemon']:
 assert package['binaries'][name]['sha256']==previous['release_binaries'][name]['sha256']
native_debug=read(BASE/'native-debug/managed-games-native.json')
assert native_debug['result']=='passed' and native_debug['cleanup_complete'] and len(native_debug['checks_passed'])==20
assert Path(native_debug['cli'])==ROOT/'target/debug/omen-cli'
copy(BASE/'native-debug/managed-games-native.json',OUT/'native-debug.json');copy(BASE/'native-debug.log',OUT/'native-debug.log')
backend_entries=[e for e in read(ROOT/'docs/evidence/launch-issues/source-manifest.json')['files'] if e['path'].startswith('crates/')]
assert len(backend_entries)==65 and all(sha(ROOT/e['path'])==e['sha256'] for e in backend_entries)
preserved={}
for version,expected in [('15','c8d546e4f2f52fed11a648819602e0d32ca550285f3f185bb8bd1138f0740fd5'),
 ('16','f83ff12dbdcb73ffa0a79c5d8bc004148773405546cf8e97b8ac8985b97767af'),
 ('17','c202e767f998a143e78f59ce3264fd429c19fd4f8fd190f1f61ebec7520186d3'),
 ('18','b6484a11f92e40c9ef18f67d7b07490a5e5af071ec412291cfc13fa40bd3e767')]:
 p=ROOT/f'target/debian/omen-linux_0.1.0-{version}_amd64.deb';assert sha(p)==expected
 preserved[version]={'path':str(p.relative_to(ROOT)),'sha256':expected}
sources=[ROOT/'Cargo.toml',ROOT/'Cargo.lock']
for directory in ['crates','gui','tests/integration']:
 base=ROOT/directory;sources+=list(base.rglob('Cargo.toml'))+[p for p in base.rglob('*.rs') if 'src' in p.relative_to(base).parts]
sources += [p for p in (ROOT/'packaging').rglob('*') if p.is_file()]
harness=[p for directory in ['tests','scripts'] for p in (ROOT/directory).rglob('*')
         if p.is_file() and '__pycache__' not in p.parts and p.suffix in ['.py','.rs','.sh','.toml']]
harness += [p for p in (ROOT/'crates').rglob('*.rs') if 'tests' in p.relative_to(ROOT/'crates').parts]
for digest,path in read(ROOT/'target/a11y-historical-recovery.json')['expected'].items():assert sha(ROOT/path)==digest
source_sha=manifest(sources,'source-manifest.json');harness_sha=manifest(harness,'test-harness-manifest.json')
doc_sha=manifest([ROOT/'README.md']+list((ROOT/'docs').glob('*.md')),'documentation-manifest.json')
summary={'recorded_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'revision':'0.1.0-19',
 'status':'verified keyboard/presentation milestone; full application audit remains incomplete',
 'language':{'application':'English only','additional_translations_in_scope':False},
 'source_manifest':'source-manifest.json','source_sha256':source_sha,
 'test_harness_manifest':'test-harness-manifest.json','test_harness_sha256':harness_sha,
 'documentation_manifest':'documentation-manifest.json','documentation_sha256':doc_sha,
 'rust_workspace':rust,'format_check':'passed','strict_clippy':'workspace all-targets all-features -D warnings passed',
 'release_binaries':package['binaries'],'runtime':read(BASE/'runtime-versions.json'),
 'keyboard_and_orca':keyboard,'native_ui':ui,
 'presentation':{'default_scenes':36,'selected_network_scenes':2,'default_text_contrast_samples':1041,
   'selected_state_surrounding_text_samples':59,'horizontal_clipping_detected':0,'collapsed_text_allocations':0,'measured_contrast_failures':0,
   'cases':['light','dark','high contrast','Sans 20 / narrow light'],'minimum_default_font':'Sans 10',
   'measurement':'Pango geometry and computed foreground/effective opacity; modal rendered background within visible uniform label allocations',
   'excluded':'Unsupported/nonuniform/partially visible contrast samples, native selection foreground, non-label input text, all unvisited/transient states',
   'full_accessibility_or_contrast_certification':False,'captures':captures},
 'visual_review':{'batch_count':1,'confirmation_batches':0,'needed_UI_corrections':False,
   'reviewed':['presentation/dark-dashboard.png','presentation/large-light-settings.png','network-selection/wide-light.png','network-selection/narrow-large-light.png'],
   'scope':'Four final own-app wide/narrow captures inspected together once; no further visual edits'},
 'additional_native_captures':native_captures,
 'historical_capture_integrity':read(ROOT/'target/a11y-historical-recovery.json'),
 'package':{'path':package['package'],'sha256':package['sha256'],'static_validation':'package-validation.json',
   'matches_final_verified_executables':True,'installed_by_verifier':False,'container_or_installed_VM_validated':False},
 'managed_debug_integration':{'checks_passed':20,'evidence':'native-debug.json','cli_sha256':sha(ROOT/'target/debug/omen-cli'),'helper_sha256':sha(ROOT/'target/debug/deps/managed_games_native-08ea28be26f13b92')},
 'backend_source_equivalence':{'compared_files':65,'changed':0,'reference':'../launch-issues/source-manifest.json'},
 'carried_evidence':{'source':'../launch-issues/validation-summary.json','sha256':sha(ROOT/'docs/evidence/launch-issues/validation-summary.json'),
   'release_CLI_and_daemon_byte_identical':True,'release_managed_integration_checks':20,'manual_launch_checks':16,'process_smoke_checks':7,
   'new_runs_of_these_backend_checks_claimed':False},
 'earlier_timeout':{'failed':'initial-library-timeout.json','same_binary_isolated_pass':'initial-library-isolated.json',
   'final_binary_sequential_runs':'library-x11.json and library-wayland.json','load_as_cause_established':False},
 'cleanup':{'native_fixture_processes_and_temporary_directories_removed':True,
   'remaining_system_managed_dump':{'path':'/var/lib/systemd/coredump/core.at-spi-bus-laun.1000.5b4da494bc594ba0be135cf635baebd9.3741191.1793100245000000.zst',
      'bytes':103860,'owner':'root','trigger':'unexpected abort when probing installed at-spi-bus-launcher --help','unprivileged_removal_refused':True,
      'privilege_changes_or_unrelated_dumps_modified':False}},
 'preserved_packages':preserved,'physical_hardware_writes_performed':False,'host_package_or_service_installed':False,
 'remaining':['Expanded/transient/hover/pressed presentation states, other themes/accents and all input-text/selection contrast',
   'Other screen readers, audible synthesis, physical keyboard/focus and effective display DPI',
   'Real native Steam/Proton, Heroic and Lutris execution; managed Flatpak and complete job-control compatibility',
   'Broader launcher formats and actual routing evidence','Current revision installed-container/VM and graphical distribution installation',
   'Physical graphical authorization, global shortcuts, compositor/full-screen overlay and suspend/resume',
   'Physical fan/RGB/charging/graphics release gates']}
summary['evidence_files']=[{'path':str(p.relative_to(OUT)),'sha256':sha(p)} for p in sorted(OUT.rglob('*')) if p.is_file() and p.name!='validation-summary.json']
(OUT/'validation-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({k:summary[k] for k in ['status','rust_workspace','keyboard_and_orca','native_ui','package']},indent=2))
