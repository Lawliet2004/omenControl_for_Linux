from pathlib import Path
import datetime, hashlib, json, os, subprocess

r=Path.cwd();out=r/'target/launcher-expansion';env=dict(os.environ)
env['LD_LIBRARY_PATH']='/home/papan/gtk-prefix/root/usr/lib/x86_64-linux-gnu'+(':'+env['LD_LIBRARY_PATH'] if env.get('LD_LIBRARY_PATH') else '')
helpers=json.loads((out/'helper-paths.json').read_text())
result={'result':'running','started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'jobs':[],
        'gui_sha256':hashlib.file_digest((r/'target/release/omen-gui').open('rb'),'sha256').hexdigest()}
if os.environ.get('OMEN_NATIVE_RESUME') == '1':
 previous=json.loads((out/'native-runner.json').read_text())
 assert previous['gui_sha256']==result['gui_sha256']
 result['jobs']=[job for job in previous['jobs'] if job['exit_code']==0]
 result['resumed_exact_binary_passes']= [job['name'] for job in result['jobs']]
def run(name,script,profile='release',backend=None):
 if any(job['name']==name and job['exit_code']==0 for job in result['jobs']):return
 e=dict(env,OMEN_TEST_PROFILE=profile,OMEN_TEST_EVIDENCE_DIR=str(out/name),OMEN_SKIP_CAPTURES='1',OMEN_NATIVE_GAME_TEST_BIN=helpers[profile])
 if backend:e['GDK_BACKEND']=backend
 with (out/(name+'.log')).open('w') as log:
  p=subprocess.run(['python3','tests/'+script+'.py'],env=e,stdout=log,stderr=subprocess.STDOUT)
 result['jobs'].append({'name':name,'script':script,'profile':profile,'backend':backend,'exit_code':p.returncode})
 (out/'native-runner.json').write_text(json.dumps(result,indent=2)+'\n')
 p.check_returncode();print(name,'passed',flush=True)
try:
 for profile in ['debug','release']:
  run('ui-'+profile+'-current','gui_game_formats',profile)
  run('keyboard-'+profile,'gui_accessibility',profile)
 run('presentation','gui_game_formats_presentation')
 for backend in ['x11','wayland']:
  for name,script in [('native','gui_settings'),('library','gui_library'),('managed-ui','gui_game_sessions'),('issues-ui','gui_launch_issues'),('shortcuts','shortcuts')]:
   run(name+'-'+backend,script,backend=backend)
 result['result']='passed'
finally:
 if result['result']!='passed':result['result']='failed'
 result['finished_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
 (out/'native-runner.json').write_text(json.dumps(result,indent=2)+'\n')
