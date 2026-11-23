#!/usr/bin/env python3
"""Publish the nested-cgroup development milestone once, with exact attribution."""
import gzip,hashlib,io,json,re,shutil,subprocess,tarfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
RAW=ROOT/'target/flatpak-nested-bridge'
OUT=ROOT/'docs/evidence/flatpak-nested-bridge'
def sha(path):
 with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def read(path):return json.loads(path.read_text())
def copy(source,relative):
 target=OUT/relative;assert source.is_file() and not source.is_symlink() and not target.exists(),(source,target)
 target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(source,target)
def tree(source,relative):
 for path in sorted(source.rglob('*')):
  if path.is_file():copy(path,Path(relative)/path.relative_to(source))
def write(relative,data):
 target=OUT/relative;assert not target.exists();target.parent.mkdir(parents=True,exist_ok=True);target.write_text(json.dumps(data,indent=2)+'\n')
def main():
 assert not OUT.exists(),'published evidence is immutable'
 vm=read(RAW/'wrapper-attempt-2/runtime/vm-systemd.json');flow=vm['flatpak_wrapper_probe']
 assert vm['result']=='passed' and len(vm['checks_passed'])==45 and vm['vm_exited'] and vm['temporary_files_removed']
 assert flow['result']=='passed' and len(flow['checks_passed'])==12 and flow['installed_cli_restored'] and flow['fixture_cleanup_complete']
 assert [x['returncode'] for x in flow['signal_results']]==[143,143]
 resource=flow['resource_policy'];assert resource['policy_before_and_after']=={'memory.max':'268435456','cpu.max':'50000 100000','pids.max':'64'}
 assert resource['child'].startswith('/sys/fs/cgroup'+resource['parent']+'/omen-game-')
 assert all(resource['child_controller_files'][key] is None for key in ['memory.max','cpu.max','pids.max'])
 assert '/omen-game-' not in flow['automatic_off_output']
 captured=flow['detached_observation']['output']
 assert re.search(r'^LAUNCHER_CWD=(.+)$',captured,re.M).group(1)==re.search(r'^GAME_CWD=(.+)$',captured,re.M).group(1)
 builds={}
 for profile in ['debug','release']:
  builds[profile]=sum(map(int,re.findall(r'test result: ok\. (\d+) passed',(RAW/f'rust-{profile}-1.log').read_text())))
  assert builds[profile]==278
 assert 'Finished' in (RAW/'clippy-1.log').read_text() and not (RAW/'fmt-1.log').read_text()
 assert read(RAW/'lock-comparison.json')['exactly_matches_revision_25']
 artifacts={
  'omen-cli-1':('70a59e6ccdc37cc3dd907e31af7a9e81054cc5178ec143e0da06038fe2a9f562',flow['development_cli_sha256']),
  'omen-flatpak-game-1':('f11b3d68396a8b30d331cf39499b66a74bb15260e41b7c220287e519f964c853',flow['wrapper_sha256']),
  'managed-games-native-1':('151c0c32947632d88dea295ef2a7bf29fcced95fc8721cf4a3358984ed27028a',flow['helper_sha256'])}
 for name,(expected,observed) in artifacts.items():assert sha(RAW/'artifacts'/name)==expected==observed
 assert sha(RAW/'artifacts/omen-flatpak-game-reproduced')==artifacts['omen-flatpak-game-1'][0]
 assert sha(ROOT/'packaging/omen-flatpak-game.c')==read(RAW/'artifacts/omen-flatpak-game-reproduced.json')['source_sha256']
 assert sha(ROOT/'tests/flatpak_wrapper_vm_inner.py')==flow['source_sha256']
 for source in (RAW/'wrapper-attempt-2/test-inputs').iterdir():assert sha(source)==sha(ROOT/'tests'/source.name)
 base=ROOT/'target/debian/omen-linux_0.1.0-25_amd64.deb'
 assert sha(base)==vm['package_sha256']=='c84e5e60bc2e0b7254cf0430e68030c0e2681ff13913c39336621fe7068f5e54'
 OUT.mkdir(parents=True);copy(Path(__file__),'collect-evidence.py');copy(ROOT/'LICENSE','LICENSE')
 for name in artifacts:copy(RAW/'artifacts'/name,Path('artifacts')/name)
 copy(RAW/'artifacts/omen-flatpak-game-reproduced.json','artifacts/wrapper-build.json')
 copy(RAW/'musl/root/usr/share/doc/musl/copyright','licenses/musl-copyright')
 with tarfile.open(fileobj=io.BytesIO(subprocess.check_output(['dpkg-deb','--fsys-tarfile',base]))) as archive:
  raw=archive.extractfile('./usr/share/doc/omen-linux/DEPENDENCY_NOTICES.md.gz').read();assert gzip.decompress(raw);(OUT/'DEPENDENCY_NOTICES.md.gz').write_bytes(raw)
 paths={v['path'] for v in read(ROOT/'docs/evidence/flatpak-bridge-development/source-manifest.json')}
 paths.remove('packaging/omen-flatpak-game');paths.update(['packaging/omen-flatpak-game.c','packaging/build-flatpak-wrapper.py'])
 sources=[]
 for relative in sorted(paths):copy(ROOT/relative,Path('sources')/relative);sources.append({'path':relative,'sha256':sha(ROOT/relative)})
 assert len(sources)==135;write('source-manifest.json',sources)
 tests=[]
 for relative in ['tests/systemd_vm.py','tests/systemd_vm_inner.py','tests/coarse_recovery_vm_inner.py','tests/nvidia_reader_vm_inner.py','tests/managed_games_vm_inner.py','tests/flatpak_scope_vm_inner.py','tests/flatpak_wrapper_vm_inner.py','scripts/prepare-offline-vm-repository.py']:
  copy(ROOT/relative,Path('sources')/relative);tests.append({'path':relative,'sha256':sha(ROOT/relative)})
 write('test-manifest.json',tests)
 documents=[]
 for name in ['FLATPAK_NESTED_BRIDGE_2026-10-28.md','FLATPAK_BRIDGE_DEVELOPMENT_2026-10-28.md','APPLICATION_AUDIT_IMPLEMENTATION.md','IMPLEMENTATION_STATUS.md','CONTINUATION_CHECKLIST.md','TEST_REPORT.md','MANAGED_GAME_SESSIONS.md']:
  relative='docs/'+name;copy(ROOT/relative,Path('reports')/relative);documents.append({'path':relative,'sha256':sha(ROOT/relative)})
 write('documentation-manifest.json',documents)
 for name in ['capability-attempt-1','wrapper-attempt-1','wrapper-attempt-2']:tree(RAW/name,Path('runs')/name)
 for path in sorted(RAW.iterdir()):
  if path.is_file() and path.suffix in ['.log','.json']:copy(path,Path('runs')/path.name)
 copy(RAW/'verify-host.py','runs/verify-host.py');copy(RAW/'musl/manifest.json','inputs/musl-manifest.json')
 for path in sorted((RAW/'musl/archives').glob('*.deb')):copy(path,Path('inputs/musl-archives')/path.name)
 repo=ROOT/'target/flatpak-bridge-development/spawn-repository'
 copy(repo/'repository-manifest.json','inputs/flatpak-repository-manifest.json')
 copy(repo/'repository/dists/resolute/InRelease','inputs/musl-InRelease')
 (OUT/'README.md').write_text('''# Nested Flatpak bridge development

Unreleased revision 26; automatic GUI setup is disabled and no package is delivered.
See reports/docs/FLATPAK_NESTED_BRIDGE_2026-10-28.md for exact scope and open work.

The passing VM run has 45 checks, including 12 Flatpak workflows and two reboots.
It uses finalized revision 25 as its base, a separately retained development CLI,
a native static sandbox wrapper, and a newly compiled root-service test helper.
The guest CLI is restored before reboot checks. The root service retains its
installed sandbox and normal polkit policy; its profile provider is private.

A failed test-assumption run and a preliminary capability run are separately
retained. Do not add their counts to the passing suite. No real Flatpak game,
Flathub launcher, GUI, physical or full job-control compatibility is claimed.

Artifacts include full project sources/license, existing Rust dependency notices,
static musl copyright and build manifest. Exact musl archives and signed-release
metadata are retained; the full signed index, offline repository and cloud image
remain in target/ at the hashes recorded in the input/run manifests. No host
package installation, service replacement or hardware write occurred.
''')
 files=[{'path':str(p.relative_to(OUT)),'bytes':p.stat().st_size,'sha256':sha(p)} for p in sorted(OUT.rglob('*')) if p.is_file()]
 summary={'result':'development_fixture_verified','release_delivered':False,'intended_revision':26,'automatic_flatpak_gui_enabled':False,'real_flatpak_game_tested':False,'real_store_launcher_tested':False,'physical_hardware_writes':False,'base_package':str(base.relative_to(ROOT)),'base_package_sha256':sha(base),'artifacts':{name:expected for name,(expected,_) in artifacts.items()},'rust_tests':builds,'installed_vm_checks':45,'flatpak_workflows':12,'two_guest_reboots':True,'guest_cli_restored':True,'fixture_cleanup_complete':True,'original_app_scope_limits_retained':resource['policy_before_and_after'],'paired_sigterm_status':143,'inherited_ignored_sigchld_tested':True,'automatic_off_tested':True,'source_files':135,'test_inputs':8,'documents':7,'host_preservation':read(RAW/'host-preservation.json'),'files':files}
 write('validation-summary.json',summary)
 print(json.dumps({'files':len(files),'summary_sha256':sha(OUT/'validation-summary.json')},indent=2))
if __name__=='__main__':main()
