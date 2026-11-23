import hashlib,json,os,subprocess,time
from pathlib import Path
root=Path.cwd();out=root/'target/a11y-final';out.mkdir(exist_ok=True)
gui=root/'target/release/omen-gui'
digest=hashlib.file_digest(gui.open('rb'),'sha256').hexdigest()
base=dict(os.environ,OMEN_TEST_PROFILE='release',OMEN_NATIVE_GAME_TEST_BIN=str(root/'target/release/deps/managed_games_native-50cfcaebb87f067b'))
assert Path(base['OMEN_NATIVE_GAME_TEST_BIN']).is_file()
jobs=[('keyboard','gui_accessibility.py',{}),('presentation','gui_presentation.py',{})]
for backend in ['x11','wayland']:
 for name,script in [('settings','gui_settings.py'),('library','gui_library.py'),('managed','gui_game_sessions.py'),('issues','gui_launch_issues.py'),('shortcuts','shortcuts.py')]:
  jobs.append((name+'-'+backend,script,{'GDK_BACKEND':backend}))
results={'gui_sha256':digest,'result':'running','jobs':[]}
for name,script,extra in jobs:
 assert hashlib.file_digest(gui.open('rb'),'sha256').hexdigest()==digest
 start=time.monotonic();print('START',name,flush=True)
 env=dict(base,OMEN_TEST_EVIDENCE_DIR=str(out/name),**extra)
 with (out/(name+'.log')).open('w') as log:
  r=subprocess.run(['python3',str(root/'tests'/script)],env=env,stdout=log,stderr=subprocess.STDOUT)
 results['jobs'].append({'name':name,'script':'tests/'+script,'exit_code':r.returncode,'seconds':time.monotonic()-start})
 print('END',name,r.returncode,flush=True)
 if r.returncode:
  results['result']='failed';(out/'runner.json').write_text(json.dumps(results,indent=2)+'\n');raise SystemExit(r.returncode)
 (out/'runner.json').write_text(json.dumps(results,indent=2)+'\n')
results['result']='passed';(out/'runner.json').write_text(json.dumps(results,indent=2)+'\n')
