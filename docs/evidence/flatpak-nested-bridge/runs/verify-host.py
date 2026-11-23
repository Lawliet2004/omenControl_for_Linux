import hashlib,json,subprocess
from pathlib import Path
root=Path.cwd();raw=root/'target/flatpak-nested-bridge'
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def read(p):return json.loads(p.read_text())
previous=read(root/'target/flatpak-bridge-development/host-preservation.json')
for p in previous['protected_processes']:
 text=(Path('/proc')/str(p['pid'])/'stat').read_text();assert int(text.rsplit(')',1)[1].split()[19])==p['start_ticks']
 command=(Path('/proc')/str(p['pid'])/'cmdline').read_bytes().replace(b'\0',b' ').decode().strip();assert command==p['command']
container=json.loads(subprocess.check_output(['docker','inspect','86ebefddca73'],text=True))[0];assert container['Id']=='86ebefddca73f91adea91f764e878e1b3ae94ab7f45069fb9cab17281c19e911'
assert all(container['State'][k]==v for k,v in previous['protected_container'].items())
for version,expected in previous['prior_packages'].items():
 path=root/f'target/debian/omen-linux_0.1.0-{version}_amd64.deb';assert path.stat().st_size==expected['bytes'] and sha(path)==expected['sha256']
runs=[]
for name in ['capability-attempt-1','wrapper-attempt-1','wrapper-attempt-2']:
 data=read(raw/name/'runtime/vm-systemd.json');assert data['vm_exited'] and data['temporary_files_removed']
 assert not Path('/proc',str(data['qemu_pid'])).exists()
 runs.append({k:data[k] for k in ['result','qemu_pid','vm_exited','temporary_files_removed'] }|{'name':name})
assert len(read(raw/'capability-attempt-1/runtime/vm-systemd.json')['checks_passed'])==41
packages={}
for package in ['omen-linux','flatpak','flatpak-xdg-utils','musl','musl-dev']:
 r=subprocess.run(['dpkg-query','-W','-f=${db:Status-Status}',package],capture_output=True,text=True)
 packages[package]=r.returncode==0 and r.stdout=='installed';assert not packages[package],(package,r.stdout)
assert not list((root/'target/debian').glob('omen-linux_0.1.0-26_*.deb'))
preserved={}
for directory,expected in {
 'flatpak-bridge-development':'b96506d2d22d2b7dc6ccd02ae65bd36b5abb45fab16dcd1828a4cf5b17918a92',
 'session-migration-v25':'de7e956bed40d6ad618cd0a38ad143da404b27f15b8b2fa1f9068de058f3327f',
 'flatpak-scope-probe-v24':'1844e6c78308975f1dc8163ca0590cd8d1efee01927b30071b06d911b2e8d9da',
 'sensor-browser-v24':'579ef9c40d7964dbee2343ec71c43e95cbdabf920eac7faed41999a65dc55254',
 'notification-workflow-v24':'72b5a7f6e24e0b5b46937f63d0fe637b8a6e61975bb0fa614a0fe2bd4b9d474f'}.items():
 path=root/'docs/evidence'/directory/'validation-summary.json';assert sha(path)==expected;preserved[directory]=expected
report={'protected_processes':previous['protected_processes'],'protected_container':{k:container['State'][k] for k in previous['protected_container']},'runs':runs,'host_installation_after':packages,'prior_packages':previous['prior_packages'],'earlier_evidence_summaries':preserved,'no_revision_26_package':True}
(raw/'host-preservation.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
