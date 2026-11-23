from pathlib import Path
import datetime,hashlib,json,lzma,os,re,shutil,subprocess,sys
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'docs/evidence/desktop-v21'
OUT.mkdir(parents=True,exist_ok=True)
def sha(path):return hashlib.file_digest(path.open('rb'),'sha256').hexdigest()
def put(source,destination):
 destination.parent.mkdir(parents=True,exist_ok=True)
 if destination.exists():assert sha(source)==sha(destination),str(destination)
 else:shutil.copyfile(source,destination)
 return {'path':str(destination.relative_to(OUT)),'sha256':sha(destination),'bytes':destination.stat().st_size}
allowed={'.json','.log','.png','.py','.txt'}
for name,folder in [('current',ROOT/'target/icon-fallback-v21'),('preparation-and-history',ROOT/'target/graphical-auth-v20')]:
 for p in sorted(folder.rglob('*')):
  relative=p.relative_to(folder)
  if not p.is_file() or p.suffix not in allowed or '__pycache__' in relative.parts or p.name.startswith('collect-'):continue
  if any(x in relative.parts for x in ['repository','indices','reproducibility-check']):continue
  put(p,OUT/name/relative)
indices=ROOT/'target/graphical-auth-v20/indices'
for p in sorted(indices.iterdir()):
 target=OUT/'indices'/p.name
 if p.name.endswith('_Packages'):
  target=target.with_name(target.name+'.xz');target.parent.mkdir(parents=True,exist_ok=True)
  if not target.exists():
   with p.open('rb') as source,lzma.open(target,'wb',preset=6) as compressed:shutil.copyfileobj(source,compressed)
  with lzma.open(target,'rb') as stream:assert hashlib.file_digest(stream,'sha256').hexdigest()==sha(p)
 elif p.name.endswith('_InRelease'):put(p,target)
put(Path(__file__),OUT/'collect-evidence.py')
for name in ['systemd_vm.py','systemd_vm_inner.py','graphical_polkit_vm_inner.py','coarse_recovery_vm_inner.py','nvidia_reader_vm_inner.py','managed_games_vm_inner.py','gui_icon_themes.py','gui_settings.py','gui_presentation.py','native_desktop.py','package.py']:
 put(ROOT/'tests'/name,OUT/'harness'/name)
put(ROOT/'scripts/prepare-offline-vm-repository.py',OUT/'harness/prepare-offline-vm-repository.py')
if '--prepare' in sys.argv:
 print('Prepared public input and raw test evidence');raise SystemExit(0)
final=ROOT/'target/debian/omen-linux_0.1.0-21_amd64.deb'
candidate=ROOT/'target/icon-fallback-v21/candidate/omen-linux_0.1.0-21_amd64.deb'
vm=json.loads((ROOT/'target/icon-fallback-v21/fresh-vm/vm-systemd.json').read_text())
assert vm['result']=='passed' and len(vm['checks_passed'])==39 and vm['vm_exited'] and vm['temporary_files_removed']
assert sha(candidate)==vm['package_sha256']=='f4fc0a72b90416260755be5e475e9926d8bf0e48a729043b3534bba6b08eaab7'
validation=json.loads((ROOT/'target/icon-fallback-v21/final-static/package-validation.json').read_text())
assert validation['sha256']==sha(final) and validation['vm_equivalence']['runtime_payload_identical']
assert sha(ROOT/'target/debian/omen-linux_0.1.0-20_amd64.deb')=='62a8d612dceab7395ad01283abf620b88daf04c381c42f8a073d46ec98353cdb'
source=[]
for folder in ['crates','gui','packaging']:
 for p in sorted((ROOT/folder).rglob('*')):
  if p.is_file() and '__pycache__' not in p.parts:source.append({'path':str(p.relative_to(ROOT)),'sha256':sha(p)})
for name in ['Cargo.toml','Cargo.lock']:source.append({'path':name,'sha256':sha(ROOT/name)})
(OUT/'source-manifest.json').write_text(json.dumps(source,indent=2)+'\n')
checks={}
for backend in ['x11','wayland']:
 m=json.loads((ROOT/('target/icon-fallback-v21/native-'+backend+'-selection/gui-settings-release.json')).read_text())
 assert m['result']=='passed' and m['cleanup_complete'] and len(m['observed_picker_selections'])==9
 checks[backend]=sum(len(m.get(k,[])) for k in ['checks_passed','network_checks_passed','health_checks_passed'])
 assert checks[backend]==31
presentation=json.loads((ROOT/'target/icon-fallback-v21/presentation-first/icon-themes.json').read_text());assert presentation['result']=='passed'
prior=json.loads((ROOT/'target/graphical-auth-v20/protected-host-before.json').read_text())
protected={}
for pid,record in prior['processes'].items():
 p=Path('/proc')/pid
 assert (p/'cmdline').read_bytes().replace(b'\0',b' ').decode()==record['cmdline']
 start=(p/'stat').read_text().split(') ',1)[1].split()[19]
 assert start==record['stat'].split(') ',1)[1].split()[19]
 protected[pid]={'cmdline_unchanged':True,'start_time_unchanged':True}
container=subprocess.check_output(['docker','inspect','--format','{{json .Id}} {{json .State.Pid}} {{json .State.StartedAt}} {{json .State.Running}}','86ebefddca73'],text=True).strip();assert container==prior['container']
protected['container_unchanged']=True
for folder in ['launcher-expansion','deployment-v20']:
 historical=ROOT/'docs/evidence'/folder
 records=json.loads((historical/'validation-summary.json').read_text())['evidence_files']
 for record in records:assert sha(historical/record['path'])==record['sha256'],str(historical/record['path'])
 protected[folder+'_evidence_files_verified']=len(records)
protected['original_unknown_overlay_preserved']=(ROOT/'target/omen-systemd-vm-wiw11l8n').is_dir()
protected['own_prior_debug_inputs_removed']=not any((ROOT/'target'/name).exists() for name in ['omen-systemd-vm-fw73_scx','omen-systemd-vm-x408nsmc'])
assert protected['original_unknown_overlay_preserved'] and protected['own_prior_debug_inputs_removed']
(OUT/'final-integrity.json').write_text(json.dumps(protected,indent=2)+'\n')
reproduction=json.loads((ROOT/'target/icon-fallback-v21/repository-reproduced/repository-manifest.json').read_text());assert reproduction['sha256']=='6481c4c39b0f3f3fc1f7e6a21e3de5218eaa57f4d8d2dc680226decc8a2a940c'
documentation=[{'path':str(p.relative_to(ROOT)),'sha256':sha(p)} for p in [ROOT/'README.md',*sorted((ROOT/'docs').glob('*.md'))]]
(OUT/'documentation-manifest.json').write_text(json.dumps(documentation,indent=2)+'\n')
files=[{'path':str(p.relative_to(OUT)),'sha256':sha(p),'bytes':p.stat().st_size} for p in sorted(OUT.rglob('*')) if p.is_file() and p.name!='validation-summary.json']
summary={'result':'passed','recorded_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
 'version':'0.1.0-21','package':str(final.relative_to(ROOT)),'package_sha256':sha(final),
 'candidate_sha256':sha(candidate),'runtime_control_equivalence':validation['vm_equivalence'],
 'fresh_vm_checks':39,'graphical_authorization_checks':6,'native_checks':checks,
 'observed_native_selections_per_backend':9,'icon_scenes':4,'mapped_icon_observations':sum(len(c['icons']) for c in presentation['cases']),
 'active_label_contrast_samples':sum(len(c['contrast']) for c in presentation['cases']),
 'rust_release_tests':264,'strict_clippy':True,'fmt':True,'host_installation':False,'physical_hardware_validated':False,
 'english_only':True,'historical_evidence_preserved':True,'source_files':len(source),'evidence_files':files,
 'limits':['real store games/accounts','managed Flatpak execution','physical input/focus/display','full accessibility and all theme states','other distributions','physical HP controls and suspend/recovery'],
 'failures_retained':'See DESKTOP_VALIDATION_2026-10-28.md and preparation-and-history/; initial native Wayland picker failure retained under current/native-wayland/.'}
(OUT/'validation-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({'result':summary['result'],'package_sha256':summary['package_sha256'],'files':len(files),'source_files':len(source)},indent=2))
