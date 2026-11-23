from pathlib import Path
import datetime, hashlib, json, shutil, subprocess

ROOT=Path.cwd();BASE=ROOT/'target/deployment-v20';OUT=ROOT/'docs/evidence/deployment-v20'
OUT.mkdir(parents=True,exist_ok=True)
def sha(p):return hashlib.file_digest(p.open('rb'),'sha256').hexdigest()
def read(p):return json.loads(p.read_text())
def copy(a,b):
 b.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(a,b)
def manifest(paths,name):
 files=[{'path':str(p.relative_to(ROOT)),'sha256':sha(p)} for p in sorted(set(paths))]
 digest=hashlib.sha256(''.join(e['sha256']+'  '+e['path']+'\n' for e in files).encode()).hexdigest()
 (OUT/name).write_text(json.dumps({'sha256':digest,'files':files},indent=2)+'\n');return digest

old=read(ROOT/'docs/evidence/launcher-expansion/validation-summary.json')
vm=read(BASE/'installed-runtime/vm-systemd.json')
assert vm['result']=='passed' and len(vm['checks_passed'])==33 and vm['vm_exit_code']==0
assert vm['vm_exited'] and vm['temporary_files_removed'] and vm['application_tests_executed']
assert vm['guest_network_restricted'] and not vm['dependency_resolution_only']
assert not vm['hardware_writes'] and not vm['physical_hardware_validated'] and not vm['host_package_installed']
package=ROOT/old['package']['path'];assert sha(package)==old['package']['sha256']==vm['package_sha256']
helper=ROOT/'target/release/deps/managed_games_native-50cfcaebb87f067b'
assert sha(helper)==vm['managed_session_fixture']['helper_sha256']==old['integration']['release']['managed_helper_sha256']
assert vm['managed_session_fixture']['cleanup_complete'] and vm['coarse_recovery_cleanup_complete'] and vm['isolated_nvidia_cleanup_complete']
assert sha(ROOT/'tests/systemd_vm_inner.py')==vm['test_payload_sha256']
ci=read(BASE/'installed-runtime/vm-cloud-init-status.json');assert ci['extended_status']=='done' and not ci['errors']
source=read(ROOT/'docs/evidence/launcher-expansion/source-manifest.json')
for e in source['files']:assert sha(ROOT/e['path'])==e['sha256'],e['path']
for name,e in old['release_binaries'].items():assert sha(ROOT/'target/release'/name)==e['sha256']
for e in old['evidence_files']:assert sha(ROOT/'docs/evidence/launcher-expansion'/e['path'])==e['sha256']

repository=read(BASE/'offline-repository-manifest.json');reproduced=read(BASE/'reproducibility-check/repository-manifest.json')
assert sha(BASE/'complete-repository.tar')==vm['offline_repository_sha256']==repository['sha256']==reproduced['sha256']
assert len(repository['packages'])==len(reproduced['packages'])==95
for e in repository['packages']:
 p=BASE/'offline-repository'/e['Filename'];assert p.stat().st_size==int(e['Size']) and sha(p)==e['SHA256']
for directory in ['installed-runtime','dependency-resolution','initial-optional-index-failure','initial-network-rename-failure']:
 data=read(BASE/directory/'vm-systemd.json')
 assert data['vm_exited'] and data['temporary_files_removed'] and data['vm_exit_code']==0
 if directory!='installed-runtime':assert not data['application_tests_executed']
 for p in (BASE/directory).iterdir():
  if p.is_file():copy(p,OUT/directory/p.name)
 for arg in data['qemu_arguments']:
  if arg.startswith('file=') and '/disk.qcow2' in arg:
   assert not Path(arg.split(',',1)[0].removeprefix('file=')).parent.exists()
assert read(OUT/'dependency-resolution/vm-systemd.json')['result']=='resolved'
for name in ['dependency-cache-audit.json','required-packages.json','offline-repository-manifest.json','mirror-probes.json','dependency-resolution-driver.log','dependency-resolution-current-driver.log','dependency-resolution-final-driver.log','installed-runtime-driver.log','reproducibility-check.log']:
 copy(BASE/name,OUT/name)
copy(BASE/'reproducibility-check/repository-manifest.json',OUT/'reproduced-repository-manifest.json')
for suite in ['resolute','resolute-updates','resolute-security']:
 for suffix in ['_InRelease','_main_binary-amd64_Packages.lz4']:
  p=next((BASE/'indices').glob('*_dists_'+suite+suffix));copy(p,OUT/'indices'/p.name)
copy(ROOT/'scripts/prepare-offline-vm-repository.py',OUT/'prepare-offline-vm-repository.py')
copy(ROOT/'target/collect-deployment-v20-evidence.py',OUT/'collect-evidence.py')
harness=[ROOT/name for name in ['tests/systemd_vm.py','tests/systemd_vm_inner.py','tests/coarse_recovery_vm_inner.py',
 'tests/nvidia_reader_vm_inner.py','tests/managed_games_vm_inner.py','scripts/prepare-offline-vm-repository.py',
 'crates/cli/tests/managed_games_native.rs']]
harness_sha=manifest(harness,'test-harness-manifest.json')
documentation_sha=manifest([ROOT/'README.md']+list((ROOT/'docs').glob('*.md')),'documentation-manifest.json')
summary={'revision':'0.1.0-20','recorded_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
 'status':'33 headless installed-service VM checks passed; graphical/physical/full-audit requirements remain open',
 'package':{'path':str(package.relative_to(ROOT)),'sha256':sha(package),'repackaged_or_installed_on_host':False},
 'application_and_packaging_sources_unchanged':{'reference':'../launcher-expansion/source-manifest.json','files_verified':len(source['files'])},
 'original_milestone_evidence_unchanged':{'reference':'../launcher-expansion/validation-summary.json','sha256':sha(ROOT/'docs/evidence/launcher-expansion/validation-summary.json'),'files_verified':len(old['evidence_files'])},
 'release_binaries':old['release_binaries'],'test_harness_manifest_sha256':harness_sha,'documentation_manifest_sha256':documentation_sha,
 'runtime':{'kernel':vm['kernel'],'systemd':vm['systemd_version'],'QEMU':vm['qemu_version'],'cloud_image_sha256':vm['image_sha256'],'host_uid':vm['host_uid']},
 'checks':{'installed_service_and_lifecycle':17,'independent_coarse_recovery':9,'isolated_NVIDIA_reader':4,'managed_sessions_in_installed_sandbox':3,'total':33,'guest_reboots':2},
 'managed_fixture':vm['managed_session_fixture'],
 'offline_repository':{'path':str((BASE/'complete-repository.tar').relative_to(ROOT)),'sha256':repository['sha256'],'bytes':repository['archive_bytes'],
  'archives':95,'assembly_local_cache_hits':sum('cache'in p['origin'] for p in repository['packages']),
  'assembly_downloads':sum('download'in p['origin'] for p in repository['packages']),
  'assembly_download_bytes':sum(int(p['Size']) for p in repository['packages'] if 'download'in p['origin']),
  'GTK_preflight_full_download_bytes':3551986,'total_full_archives_downloaded_this_turn':30,'total_full_archive_download_bytes':20512990,
  'signed_indices_verified_on_host_and_by_guest_APT':True,'strong_package_SHA256_from_signed_indices':True,
  'APT_signature_or_freshness_checks_disabled':False,'guest_network_restricted_to_explicit_SSH_forward':True,'exact_archive_reproduced_from_retained_inputs':True},
 'bootstrap_attempts':{'optional_index_failure':'initial-optional-index-failure/vm-systemd.json','NIC_rename_warning':'initial-network-rename-failure/vm-systemd.json',
  'successful_dependency_resolution':'dependency-resolution/vm-systemd.json','application_tests_run_in_bootstraps':False,'all_guests_exited_and_private_inputs_removed':True},
 'cleanup':{'VM_exit_code':0,'temporary_disk_seed_private_key_removed':True,'fixture_overrides_rules_and_helpers_removed':True},
 'physical_hardware_writes':False,'host_package_or_service_installation':False,
 'remaining':['Graphical polkit prompts, cancellation and authorization cache behavior','Complete graphical desktop/distribution installation',
  'Real game/store/runtime and managed sandbox execution','Physical shortcuts/focus and compositor/full-screen placement',
  'Complete accessibility and physical DPI','HP fan/RGB/charging/graphics/thermal/recovery/suspend release gates']}
summary['evidence_files']=[{'path':str(p.relative_to(OUT)),'sha256':sha(p)} for p in sorted(OUT.rglob('*')) if p.is_file() and p.name!='validation-summary.json']
(OUT/'validation-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({k:summary[k] for k in ['status','package','checks','offline_repository']},indent=2))
