#!/usr/bin/env python3
"""Actual Flatpak/cgroup/namespace observations in a disposable VM only.

The runtime/app are explicit locally built fixtures, not a Flathub game claim.
Signed distro Flatpak, bubblewrap, CLI and production service code are used.
"""
import hashlib
import json
import os
from pathlib import Path
import pwd
import re
import shutil
import subprocess
import time

RESULT=Path('/var/tmp/omen-vm-result.json')
BASE=Path('/var/tmp/omen-flatpak-scope-probe')
APP='org.omenlinux.ScopeProbe'
PLATFORM='org.omenlinux.ScopePlatform'
HELPER=Path('/usr/local/libexec/omen-managed-fixture')


def run(*args,check=True,env=None,timeout=60):
    result=subprocess.run(args,capture_output=True,text=True,env=env,timeout=timeout)
    print('+',' '.join(args),result.returncode,result.stdout.strip(),result.stderr.strip(),flush=True)
    if check:result.check_returncode()
    return result


def wait(predicate,message,timeout=25):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        try:
            value=predicate()
            if value:return value
        except (OSError,ValueError,subprocess.SubprocessError):pass
        time.sleep(.1)
    raise RuntimeError(message)


def main():
    assert os.geteuid()==0 and run('systemd-detect-virt','--vm').stdout.strip() in ['qemu','kvm']
    assert RESULT.is_file() and HELPER.is_file() and not BASE.exists()
    state=json.loads(RESULT.read_text());account=pwd.getpwnam('ubuntu')
    BASE.mkdir();os.chown(BASE,account.pw_uid,account.pw_gid)
    env=dict(os.environ,HOME=account.pw_dir,XDG_RUNTIME_DIR='/run/user/'+str(account.pw_uid),
        DBUS_SESSION_BUS_ADDRESS='unix:path=/run/user/'+str(account.pw_uid)+'/bus',
        OMEN_CONFIG_DIR=str(BASE/'user'),OMEN_SYSTEM_CONFIG_DIR=str(BASE/'system'),LC_ALL='C.UTF-8')
    data={'result':'failed','checks_passed':[],'physical_hardware_writes':False,'real_game_tested':False,
        'runtime':'locally built explicit scope fixture; not a Flathub compatibility claim',
        'source_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'helper_sha256':hashlib.file_digest(HELPER.open('rb'),'sha256').hexdigest()}
    children=[];logs=[];instances=set()
    # This stage is restricted to a fresh disposable guest. Never remove a
    # pre-existing installation, cache or application directory during cleanup.
    created_paths=[Path(account.pw_dir)/suffix for suffix in
        ['.local/share/flatpak','.cache/flatpak','.var/app/'+APP]]
    assert all(not path.exists() for path in created_paths), 'probe requires a fresh guest Flatpak user installation'
    def args(*command):return ['runuser','-u','ubuntu','--','env',*[key+'='+value for key,value in env.items() if key in ['HOME','XDG_RUNTIME_DIR','DBUS_SESSION_BUS_ADDRESS','OMEN_CONFIG_DIR','OMEN_SYSTEM_CONFIG_DIR','LC_ALL']],*map(str,command)]
    def user(*command,check=True):return run(*args(*command),check=check)
    def start(command,name):
        log=(BASE/(name+'.log')).open('w');logs.append(log)
        process=subprocess.Popen(args(*command),stdout=log,stderr=subprocess.STDOUT,text=True)
        children.append(process);return process
    def passed(message):data['checks_passed'].append(message);print('PASS',message,flush=True)
    def running():
        lines=user('flatpak','ps','--columns=instance,pid,child-pid,application').stdout.splitlines()
        return [dict(zip(['instance','pid','child_pid','application'],line.split())) for line in lines if APP in line]
    def current():
        rows=running()
        for row in rows:instances.add(row['instance'])
        for row in rows:
            try:
                if int(row['child_pid'])>0:
                    row['host_cgroup']=Path('/proc/'+row['child_pid']+'/cgroup').read_text().strip()
                    return row
            except (OSError,ValueError):pass
        return None
    def request(op):
        script='''import json,socket,sys
with socket.socket(socket.AF_UNIX) as stream:
 stream.settimeout(6);stream.connect(sys.argv[1]);stream.sendall(json.dumps({'v':1,'id':1,'op':{'op':sys.argv[2]}}).encode()+b'\\n')
 response=json.loads(stream.makefile('rb').readline());assert response['ok'],response;print(json.dumps(response['data']))
'''
        return json.loads(user('python3','-c',script,BASE/'ctl.sock',op).stdout)
    try:
        run('loginctl','enable-linger','ubuntu');run('systemctl','start','user@'+str(account.pw_uid)+'.service')
        data['flatpak_version']=user('flatpak','--version').stdout.strip()
        data['bubblewrap_version']=user('bwrap','--version').stdout.strip()
        runtime=BASE/'runtime';app=BASE/'app';repo=BASE/'repo'
        (runtime/'usr/bin').mkdir(parents=True);(runtime/'usr/lib').mkdir();(runtime/'files').mkdir()
        (runtime/'usr/lib64').symlink_to('lib');(runtime/'metadata').write_text('[Runtime]\nname='+PLATFORM+'\n')
        libraries=set()
        for name in ['bash','cat','sleep','env','mkdir','readlink','pwd']:
            source=Path(shutil.which(name)).resolve();shutil.copyfile(source,runtime/'usr/bin'/name);(runtime/'usr/bin'/name).chmod(0o755)
            output=run('ldd',str(source)).stdout
            for line in output.splitlines():
                match=re.search(r'(/[^ ()]+)',line)
                if match:libraries.add(Path(match[1]))
        for source in libraries:shutil.copy2(source,runtime/'usr/lib'/source.name)
        (runtime/'usr/bin/sh').symlink_to('bash')
        ldconfig=Path(shutil.which('ldconfig')).resolve();shutil.copyfile(ldconfig,runtime/'usr/bin/ldconfig');(runtime/'usr/bin/ldconfig').chmod(0o755)
        (app/'files/bin').mkdir(parents=True);(app/'files/share/applications').mkdir(parents=True)
        (app/'metadata').write_text('[Application]\nname='+APP+'\nruntime='+PLATFORM+'/x86_64/stable\ncommand=scope-probe\n')
        (app/'files/share/applications'/ (APP+'.desktop')).write_text('[Desktop Entry]\nType=Application\nName=OMEN scope fixture\nExec=scope-probe\n')
        (app/'files/bin/scope-probe').write_text('#!/bin/sh\nmkdir -p "$XDG_DATA_HOME/working directory"\ncd "$XDG_DATA_HOME/working directory" || exit 20\nexport OMEN_PROBE_DYNAMIC="sandbox dynamic value"\nprintf "SANDBOX_PID=%s\\nCWD=%s\\nENV=%s\\n" "$$" "$PWD" "$OMEN_PROBE_DYNAMIC"\ncat /proc/self/cgroup\nexec /usr/bin/sleep 120\n')
        (app/'files/bin/scope-probe').chmod(0o755)
        run('chown','-R',str(account.pw_uid)+':'+str(account.pw_gid),str(BASE))
        user('flatpak','build-export','--disable-sandbox','--runtime',repo,runtime,'stable')
        user('flatpak','build-finish',app)
        user('flatpak','build-export','--disable-sandbox',repo,app,'stable')
        user('flatpak','remote-add','--user','--no-gpg-verify','omen-scope-fixture',repo)
        user('flatpak','install','--user','--assumeyes','--noninteractive','omen-scope-fixture',PLATFORM,APP)
        passed('signed distro Flatpak installs a local minimal runtime and explicit sandbox fixture without a Flathub or game claim')
        raw_unit='omen-game-'+('1'*32)+'.scope'
        raw=start(['systemd-run','--user','--scope','--collect','--quiet','--unit='+raw_unit,'--','/usr/bin/flatpak','run','--sandbox',APP],'raw-scope')
        first=wait(lambda:current() if 'ENV=sandbox dynamic value' in (BASE/'raw-scope.log').read_text() else None,
            'actual Flatpak fixture did not execute')
        group=first['host_cgroup']
        data['raw_scope']={'unit':raw_unit,'flatpak':first,'observed_cgroup':group,
            'original_unit_state':user('systemctl','--user','show',raw_unit,'-p','ActiveState','--value',check=False).stdout.strip()}
        assert 'app-flatpak-' in group and raw_unit not in group,group
        passed('real flatpak run moves its sandbox outside the initial OMEN-shaped scope into a separate app-flatpak scope')
        user('flatpak','kill',first['instance']);raw.wait(timeout=15);instances.discard(first['instance'])
        # Actual installed CLI + production service/session code, private PPD.
        (BASE/'user').mkdir();(BASE/'system').mkdir()
        (BASE/'user/config.toml').write_text('version=1\n[games]\nmanaged_game_ids=["custom/flatpak-probe"]\n[games.per_game_profiles]\n"custom/flatpak-probe"="performance"\n')
        run('chown','-R',str(account.pw_uid)+':'+str(account.pw_gid),str(BASE))
        log=(BASE/'private-bus.log').open('w');logs.append(log)
        bus=subprocess.Popen(args('dbus-daemon','--session','--nofork','--print-address=1','--address=unix:path='+str(BASE/'bus')),
            stdout=subprocess.PIPE,stderr=log,text=True);children.append(bus);address=bus.stdout.readline().strip();assert address.startswith('unix:')
        log=(BASE/'ppd.log').open('w');logs.append(log)
        ppd=subprocess.Popen(args(HELPER,'ppd-service',address),stdout=subprocess.PIPE,stderr=log,text=True);children.append(ppd);assert ppd.stdout.readline().strip()=='READY'
        log=(BASE/'service.log').open('w');logs.append(log)
        service=subprocess.Popen(args(HELPER,'fixture-service',BASE/'ctl.sock',BASE/'sessions.json',address,'mock'),stdout=subprocess.PIPE,stderr=log,text=True);children.append(service);assert service.stdout.readline().strip()=='READY'
        managed=start(['/usr/bin/omen-cli','--socket',str(BASE/'ctl.sock'),'game-run','custom/flatpak-probe','--','/usr/bin/flatpak','run','--sandbox',APP],'managed-scope')
        active=wait(current,'Flatpak did not start through the installed actual-command wrapper')
        wait(lambda:any(item['game_id']=='custom/flatpak-probe' and item['phase']=='ended' for item in request('get_game_sessions')['sessions']),
            'scope migration did not expose the expected registration/lifetime mismatch')
        assert Path('/proc/'+active['child_pid']).is_dir() and request('get_profile')=='balanced'
        data['managed_migration']={'flatpak':active,'sessions':request('get_game_sessions'),'profile_while_sandbox_alive':request('get_profile')}
        passed('installed OMEN wrapper releases the private profile after scope migration while the real sandbox remains alive; naive host wrapping is insufficient')
        # Resolve an actual sandbox command PID, rather than assuming the wrapper
        # PID has the command's dynamic environment or working directory.
        root_ns=(Path('/proc')/active['child_pid']/'ns/pid').stat().st_ino
        target=None
        for proc in Path('/proc').iterdir():
            if not proc.name.isdigit():continue
            try:
                if proc.stat().st_uid==account.pw_uid and (proc/'ns/pid').stat().st_ino==root_ns and (proc/'comm').read_text().strip()=='sleep':target=int(proc.name);break
            except OSError:pass
        assert target is not None
        enter_unit='omen-game-'+('2'*32)+'.scope'
        values=['two words','','\'quoted\'','$HOME','α','--literal']
        entered=user('systemd-run','--user','--scope','--collect','--quiet','--unit='+enter_unit,'--',
            '/usr/bin/flatpak','enter',str(target),'/usr/bin/sh','-c',
            'printf "ENV=%s\\nCWD=%s\\n" "$OMEN_PROBE_DYNAMIC" "$PWD"; cat /proc/self/cgroup; printf "ARG=%s\\0" "$@"; exit 7',
            'entry-fixture',*values,check=False)
        data['enter_observation']={'target_host_pid':target,'namespace_inode':root_ns,'expected_command_exit':7,
            'return_code':entered.returncode,'arguments':values,'stdout':entered.stdout,'stderr':entered.stderr}
        assert 'ENV=sandbox dynamic value' in entered.stdout and 'working directory' in entered.stdout and enter_unit in entered.stdout
        assert ''.join('ARG='+value+'\0' for value in values) in entered.stdout
        assert entered.returncode==0, 'pinned 1.16.6 enter exit-status observation changed; review rather than accepting a new version silently'
        passed('Flatpak 1.16.6 enter preserves exact arguments, dynamic environment, cwd and OMEN scope, but reports exit 7 as success')
        control=user('flatpak','run','--sandbox','--command=/usr/bin/sh',APP,'-c','exit 7',check=False)
        data['direct_exit_control']={'expected_command_exit':7,'return_code':control.returncode,'stdout':control.stdout,'stderr':control.stderr}
        assert control.returncode==7
        passed('the same sandbox command through ordinary flatpak run returns exit 7; the success mismatch is specific to enter')
        signaled=user('systemd-run','--user','--scope','--collect','--quiet','--unit=omen-game-'+('3'*32)+'.scope','--',
            '/usr/bin/flatpak','enter',str(target),'/usr/bin/sh','-c','kill -TERM $$',check=False)
        data['enter_signal']={'command_signal':15,'return_code':signaled.returncode,'stdout':signaled.stdout,'stderr':signaled.stderr}
        assert signaled.returncode==15
        passed('Flatpak 1.16.6 enter turns sandbox SIGTERM into numeric exit 15, losing terminating-signal semantics')
        prototype=start(['/usr/bin/omen-cli','--socket',str(BASE/'ctl.sock'),'game-run','custom/flatpak-probe','--',
            '/usr/bin/flatpak','enter',str(target),'/usr/bin/sh','-c',
            'sleep 6 & printf "DESCENDANT_STARTED\\n"; exit 7'],'entry-descendant')
        wait(lambda:'DESCENDANT_STARTED' in (BASE/'entry-descendant.log').read_text(),'namespace descendant did not execute')
        prototype.wait(timeout=10)
        tracked=next(item for item in request('get_game_sessions')['sessions'] if item['phase']=='tracking')
        assert prototype.returncode==0 and tracked['profile_held'] and request('get_profile')=='performance'
        descendant_group='omen-game-'+tracked['token']+'.scope';descendants=[]
        for proc in Path('/proc').iterdir():
            if not proc.name.isdigit():continue
            try:
                group=(proc/'cgroup').read_text().strip()
                if proc.stat().st_uid==account.pw_uid and (proc/'comm').read_text().strip()=='sleep' and descendant_group in group:
                    descendants.append({'host_pid':int(proc.name),'host_cgroup':group})
            except OSError:pass
        assert len(descendants)==1
        data['entry_descendant']={'wrapper_return_code':prototype.returncode,'tracking_session':tracked,
            'profile_after_entry_parent_exit':request('get_profile'),'live_descendants':descendants}
        wait(lambda:request('get_profile')=='balanced','namespace descendant exit did not release the private hold')
        ended=next(item for item in request('get_game_sessions')['sessions'] if item['token']==tracked['token'])
        assert ended['phase']=='ended' and not ended['profile_held'];data['entry_descendant']['final_session']=ended
        passed('namespace-entry prototype keeps a real detached sandbox descendant in the OMEN scope and private hold until final exit; exit-status defects still prevent support')
        data['result']='passed'
    except BaseException as error:
        data['error']=type(error).__name__+': '+str(error);raise
    finally:
        for instance in instances:user('flatpak','kill',instance,check=False)
        for process in reversed(children):
            if process.poll() is None:
                process.terminate()
                try:process.wait(timeout=10)
                except subprocess.TimeoutExpired:process.kill();process.wait(timeout=5)
        user('flatpak','uninstall','--user','--assumeyes',APP,PLATFORM,check=False)
        user('flatpak','remote-delete','--user','omen-scope-fixture',check=False)
        for log in logs:log.close()
        data['logs']={path.name:path.read_text() for path in BASE.glob('*.log')}
        shutil.rmtree(BASE)
        for path in created_paths:
            if path.exists():shutil.rmtree(path)
        HELPER.unlink()
        data['fixture_cleanup_complete']=not BASE.exists() and not HELPER.exists() and all(not path.exists() for path in created_paths)
        state['flatpak_scope_probe']=data
        if data['result']=='passed':state['checks_passed'].extend(data['checks_passed'])
        RESULT.write_text(json.dumps(state,indent=2)+'\n')


if __name__=='__main__':main()
