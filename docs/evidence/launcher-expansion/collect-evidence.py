from pathlib import Path
import datetime, gzip, hashlib, json, re, shutil

ROOT=Path.cwd();BASE=ROOT/'target/launcher-expansion';OUT=ROOT/'docs/evidence/launcher-expansion'
OUT.mkdir(exist_ok=True)
def sha(p):return hashlib.file_digest(p.open('rb'),'sha256').hexdigest()
def read(p):return json.loads(p.read_text())
def copy(src,dst):
 dst.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(src,dst)
def manifest(paths,name):
 entries=[{'path':str(p.relative_to(ROOT)),'sha256':sha(p)} for p in sorted(set(paths)) if p.is_file()]
 digest=hashlib.sha256(''.join(e['sha256']+'  '+e['path']+'\n' for e in entries).encode()).hexdigest()
 (OUT/name).write_text(json.dumps({'sha256':digest,'files':entries},indent=2)+'\n');return digest
def passed(data,count=None):
 assert data['result']=='passed' and data.get('cleanup_complete',True)
 if count is not None:assert len(data['checks_passed'])==count

package=read(BASE/'package-static-final/package-validation.json')
assert package['version']=='0.1.0-20' and all(package['checks'].values())
assert sha(ROOT/package['package'])==package['sha256'] and package['lifecycle_equivalence']['runtime_payload_identical']
for name,entry in package['binaries'].items():assert entry['matches_release'] and sha(ROOT/'target/release'/name)==entry['sha256']
gui=package['binaries']['omen-gui']['sha256']
runner=read(BASE/'native-runner.json');assert runner['result']=='passed' and len(runner['jobs'])==15 and all(j['exit_code']==0 for j in runner['jobs'])
assert runner['gui_sha256']==gui
copy(BASE/'native-runner.json',OUT/'native-runner.json');copy(ROOT/'target/run-launcher-native.py',OUT/'run-native-validation.py')
copy(ROOT/'target/collect-launcher-expansion-evidence.py',OUT/'collect-evidence.py')
rust={};integration={};keyboard={}
for profile in ['debug','release']:
 log=BASE/f'tests-{profile}.log';counts=[tuple(map(int,c)) for c in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',log.read_text())]
 assert sum(c[0] for c in counts)==264 and all(c[1]==c[2]==0 for c in counts)
 rust[profile]={'passed':264,'failed':0,'ignored':0};copy(log,OUT/log.name)
 formats=read(BASE/f'formats-{profile}-final/game-formats.json');passed(formats,17)
 assert formats['upstream_legendary_tested'] and formats['upstream_network_attempts']==0
 assert formats['cli_sha256']==sha(ROOT/'target'/profile/'omen-cli') and formats['daemon_sha256']==sha(ROOT/'target'/profile/'omen-daemon')
 managed=read(BASE/f'managed-{profile}/managed-games-native.json');passed(managed,20)
 ui=read(BASE/f'ui-{profile}-current/gui-game-formats.json');passed(ui,6)
 key=read(BASE/f'keyboard-{profile}/keyboard-orca.json');passed(key,13)
 assert ui['gui_sha256']==key['gui_sha256']==sha(ROOT/'target'/profile/'omen-gui')
 for folder,file,name in [(f'formats-{profile}-final','game-formats.json',f'formats-{profile}'),(f'managed-{profile}','managed-games-native.json',f'managed-{profile}'),(f'ui-{profile}-current','gui-game-formats.json',f'launcher-ui-{profile}'),(f'keyboard-{profile}','keyboard-orca.json',f'keyboard-{profile}')]:
  copy(BASE/folder/file,OUT/(name+'.json'))
  for p in (BASE/folder).glob('*.log'):
   if p.name=='orca-debug.log':(OUT/(name+'-orca.log.gz')).write_bytes(gzip.compress(p.read_bytes(),mtime=0))
   else:copy(p,OUT/(name+'-'+p.name))
  copy(BASE/(folder+'.log'),OUT/(name+'.log'))
 integration[profile]={'launcher_format_checks':17,'managed_session_checks':20,'launcher_keyboard_workflows':6,
  'cli_sha256':formats['cli_sha256'],'daemon_sha256':formats['daemon_sha256'],'managed_helper_sha256':formats['managed_helper_sha256']}
 keyboard[profile]={'workflows':13,'generated_utterances':len(key['speech_output']),'audible_synthesis_tested':False,'host_keyboard_events':False}
native={}
for backend in ['x11','wayland']:
 records={}
 for folder,file in [('native', 'gui-settings-release.json'),('library','library-ui.json'),('managed-ui','gui-game-sessions-release.json'),('issues-ui','gui-launch-issues-release.json'),('shortcuts',f'shortcuts-release-{backend}.json')]:
  path=BASE/f'{folder}-{backend}';data=read(path/file);passed(data)
  assert data.get('gui_sha256',data.get('binary_sha256'))==gui
  records[folder]=data;copy(path/file,OUT/f'{folder}-{backend}.json');copy(BASE/f'{folder}-{backend}.log',OUT/f'{folder}-{backend}.log')
  for p in path.glob('*.log'):copy(p,OUT/f'{folder}-{backend}-{p.name}')
 s,l,m,i,k=[records[n] for n in ['native','library','managed-ui','issues-ui','shortcuts']]
 assert sum(len(s[key]) for key in ['checks_passed','network_checks_passed','health_checks_passed'])==31
 assert l['count']==10000 and len(l['checks_passed'])==5 and len(m['checks_passed'])==5 and len(i['checks_passed'])==6 and len(k['checks_passed'])==32
 native[backend]={'native_regressions':31,'library_entries':10000,'library_workflows':5,'managed_workflows':5,'launch_issue_workflows':6,'shortcut_overlay_checks':32,
  'scan_and_first_accessible_response_seconds':l['scan_and_first_accessible_response_seconds'],'loaded_rss_kib':l['loaded_rss_kib'],'peak_observed_rss_kib':l['peak_observed_rss_kib']}
presentation=read(BASE/'presentation/presentation.json');passed(presentation);assert presentation['gui_sha256']==gui and len(presentation['cases'])==4
captures=[]
for case in presentation['cases']:
 assert all(not case[key] for key in ['horizontal_clipping','contrast_failures','collapsed_text_allocations'])
 p=BASE/'presentation'/case['capture'];assert sha(p)==case['capture_sha256']
 dest=ROOT/'docs/screenshots/launcher-expansion'/p.name;copy(p,dest)
 copy(BASE/'presentation'/case['report'],OUT/'presentation'/case['report'])
 captures.append({'path':str(dest.relative_to(ROOT)),'sha256':sha(dest),'case':case['case']})
copy(BASE/'presentation/presentation.json',OUT/'presentation/presentation.json');copy(BASE/'presentation.log',OUT/'presentation.log')
lifecycle=read(BASE/'package-lifecycle/package-lifecycle.json');assert lifecycle['result']=='passed' and lifecycle['container_removed'] and len(lifecycle['checks_passed'])==20
installed=read(BASE/'package-lifecycle/installed-game-formats.json');passed(installed,13);assert not installed['upstream_legendary_tested']
assert installed['cli_sha256']==package['binaries']['omen-cli']['sha256'] and installed['daemon_sha256']==package['binaries']['omen-daemon']['sha256']
snapshot=read(BASE/'dependency-snapshot.json');assert snapshot['temporary_image_removed'] and snapshot['source_container_unchanged'] and not snapshot['snapshot_paused_source']
for directory in ['package-lifecycle','package-static-final']:
 for p in (BASE/directory).iterdir():
  if p.is_file():copy(p,OUT/p.name)
for name in ['dependency-snapshot.json','test-dependency.json','upstream-archive.json','format.log','clippy.log','game_launch.log','smoke.log','package-final-build.log','package-static-final.log']:
 copy(BASE/name,OUT/name)
assert len(read(BASE/'game_launch.log')['checks_passed'])==16 and len(read(BASE/'smoke.log')['checks_passed'])==7
for folder in ['package-lifecycle-apt-path-failure','package-lifecycle-wrong-baseline-failure','ui-debug-selector-failure','ui-debug-end-selection-failure','ui-debug-arrow-selection-failure','ui-debug-focus-selection-failure','ui-debug-input-focus-failure']:
 path=BASE/folder
 for p in path.glob('*.json'):copy(p,OUT/'initial-harness-failures'/folder/p.name)
 for p in path.glob('*.log'):copy(p,OUT/'initial-harness-failures'/folder/p.name)
copy(BASE/'probe-selector.log',OUT/'initial-harness-failures/native-popup-focus-selection.log')
for name in ['native-runner-presentation-staging-failure.json','native-runner-presentation-search-string-failure.json']:
 copy(BASE/name,OUT/'initial-harness-failures'/name)
before=BASE/'before-location-alignment/presentation'
copy(before/'presentation.json',OUT/'presentation/before-location-alignment.json')
copy(before/'locations-narrow-large-dark.png',ROOT/'docs/screenshots/launcher-expansion/before-locations-narrow-large-dark.png')
copy(BASE/'before-location-alignment/dependency-snapshot.json',OUT/'previous-dependency-snapshot.json')
preserved={}
for revision,digest in [('15','c8d546e4f2f52fed11a648819602e0d32ca550285f3f185bb8bd1138f0740fd5'),('16','f83ff12dbdcb73ffa0a79c5d8bc004148773405546cf8e97b8ac8985b97767af'),('17','c202e767f998a143e78f59ce3264fd429c19fd4f8fd190f1f61ebec7520186d3'),('18','b6484a11f92e40c9ef18f67d7b07490a5e5af071ec412291cfc13fa40bd3e767'),('19','0fa1a2209946d4b82aa15cc119dac3167f6165fb1717284cf43e7ed59448dee6')]:
 p=ROOT/f'target/debian/omen-linux_0.1.0-{revision}_amd64.deb';assert sha(p)==digest;preserved[revision]={'path':str(p.relative_to(ROOT)),'sha256':digest}
sources=[ROOT/'Cargo.toml',ROOT/'Cargo.lock']
for directory in ['crates','gui','tests/integration']:
 base=ROOT/directory;sources+=list(base.rglob('Cargo.toml'))+[p for p in base.rglob('*.rs') if 'src' in p.relative_to(base).parts]
sources += [p for p in (ROOT/'packaging').rglob('*') if p.is_file()]
harness=[p for directory in ['tests','scripts'] for p in (ROOT/directory).rglob('*') if p.is_file() and '__pycache__' not in p.parts and p.suffix in ['.py','.rs','.sh','.toml']]
harness += [p for p in (ROOT/'crates').rglob('*.rs') if 'tests' in p.relative_to(ROOT/'crates').parts]
summary={'revision':'0.1.0-20','recorded_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
 'status':'verified launcher/keyboard/installed-package milestone; full audit remains incomplete','application_language':'English only',
 'source_manifest_sha256':manifest(sources,'source-manifest.json'),'test_harness_manifest_sha256':manifest(harness,'test-harness-manifest.json'),
 'documentation_manifest_sha256':manifest([ROOT/'README.md']+list((ROOT/'docs').glob('*.md')),'documentation-manifest.json'),
 'rust_workspace':rust,'format_check':'passed','strict_all_target_all_feature_clippy':'passed','release_binaries':package['binaries'],
 'integration':integration,'keyboard_and_orca':keyboard,'native_ui':native,
 'presentation':{'scenes':4,'text_label_contrast_samples':sum(len(c['contrast']) for c in presentation['cases']),
  'horizontal_clipping_detected':0,'collapsed_text_allocations':0,'measured_contrast_failures':0,
  'captures':captures,'measurement':'Actual Pango geometry, computed foreground/effective opacity and sampled uniform rendered backgrounds; fully visible labels only',
  'full_accessibility_certification':False},
 'visual_review':{'batch_count':2,'confirmation_batches':1,'scope':'Four final wide/light and narrow/Sans 20/dark expanded launcher/location captures',
  'correction':'Center Remove buttons beside wrapped paths and replace the long all-launcher paragraph with selected-launcher folder guidance',
  'before_capture':'docs/screenshots/launcher-expansion/before-locations-narrow-large-dark.png'},
 'upstream_legendary':{'commit':read(OUT/'legendary-tree.json')['commit'],'verified_archive_files':len(read(OUT/'legendary-tree.json')['files']),
  'synthetic_offline_games':True,'actual_command_wrapper_and_user_scope':True,'outbound_attempts':0,'real_Epic_account_or_game_tested':False},
 'manual_launch_checks':16,'process_smoke_checks':7,
 'package':{'path':package['package'],'sha256':package['sha256'],'lifecycle_candidate_sha256':lifecycle['package_sha256'],
  'documentation_only_equivalence':package['lifecycle_equivalence'],'installed_lifecycle_checks':20,'installed_format_checks':13,
  'reused_distribution_dependencies':True,'pristine_distribution_resolution_claimed':False,'systemd_VM_tested':False},
 'cleanup':{'fixture_processes_and_directories_removed':True,'package_test_container_removed':True,'dependency_image_removed':True,'original_dependency_container_unchanged':True},
 'physical_hardware_writes':False,'host_package_or_service_installed':False,'preserved_packages':preserved,
 'remaining':['Real Steam/Proton, Heroic, Lutris and Epic/Snap game/account runtime validation','Managed sandbox execution and complete job-control behavior',
  'Heroic sideload/Zoom/third-party formats and other launcher installations','Complete accessibility/state/theme/input/selection contrast and physical focus/DPI',
  'Current systemd VM, graphical authorization and distribution installation','Physical fan/RGB/charging/graphics/suspend and desktop shortcut/full-screen overlay gates']}
summary['evidence_files']=[{'path':str(p.relative_to(OUT)),'sha256':sha(p)} for p in sorted(OUT.rglob('*')) if p.is_file() and p.name!='validation-summary.json']
(OUT/'validation-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({k:summary[k] for k in ['status','rust_workspace','presentation','package']},indent=2))
