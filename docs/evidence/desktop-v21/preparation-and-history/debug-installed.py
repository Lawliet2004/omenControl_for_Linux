from pathlib import Path
import ast, hashlib, json, os, re, shutil, socket, subprocess, sys, time
ROOT = Path(__file__).resolve().parents[2]
base = Path(__file__).resolve().parent
original = json.loads((base/'installed-attempt-2/vm-systemd.json').read_text())
work = Path(original['retained_work_directory'])
assert work == ROOT/'target/omen-systemd-vm-x408nsmc' and original['vm_exited'] and original['vm_exit_code']==0
assert os.geteuid() != 0 and work.is_dir()
for process in Path('/proc').iterdir():
    if process.name.isdigit():
        try:
            raw=(process/'cmdline').read_bytes()
            assert not (b'qemu-system' in raw and str(work/'disk.qcow2').encode() in raw), 'private VM is already running'
        except (FileNotFoundError, ProcessLookupError, PermissionError): pass
EVIDENCE=base/sys.argv[1];EVIDENCE.mkdir(exist_ok=False)
source=ROOT/'tests/systemd_vm.py'
shutil.copyfile(source,EVIDENCE/'systemd_vm_source.py')
shutil.copyfile(ROOT/'tests/graphical_polkit_vm_inner.py',EVIDENCE/'graphical_source.py')
result={'result':'failed','reused_installed_guest':True,'fresh_install':False,'full_systemd_suite_executed':False,
        'package_sha256':original['package_sha256'],'harness_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),
        'graphical_source_sha256':hashlib.sha256((EVIDENCE/'graphical_source.py').read_bytes()).hexdigest(),
        'host_package_installed':False,'physical_hardware_writes':False}
arguments=original['qemu_arguments'][:]
serial=arguments.index('-serial')+1;arguments[serial]='file:'+str(EVIDENCE/'serial.log')
command=[str(ROOT/'target/vm-runtime/root/usr/bin/qemu-system-x86_64'),*arguments]
port=re.search(r'hostfwd=tcp:127\.0\.0\.1:(\d+)-:22',arguments[arguments.index('-netdev')+1])[1]
options=['-i',str(work/'id_ed25519'),'-o','BatchMode=yes','-o','ConnectTimeout=3','-o','StrictHostKeyChecking=accept-new','-o','UserKnownHostsFile='+str(work/'known_hosts'),'-o','LogLevel=ERROR']
ssh_base=['ssh',*options,'-p',port,'ubuntu@127.0.0.1']
env=dict(os.environ,LD_LIBRARY_PATH=str(ROOT/'target/vm-runtime/root/usr/lib/x86_64-linux-gnu'),QEMU_MODULE_DIR=str(ROOT/'target/vm-runtime/root/usr/lib/x86_64-linux-gnu/qemu'))
log=(EVIDENCE/'vm-systemd-run.log').open('w')
def ssh(remote,timeout=30,check=True):
    log.write('+ '+remote+'\n');log.flush()
    value=subprocess.run([*ssh_base,remote],capture_output=True,text=True,timeout=timeout)
    log.write(value.stdout+value.stderr);log.flush()
    if check:value.check_returncode()
    return value

def copy(source,target):
    subprocess.run(['scp','-q',*options,'-P',port,str(source),'ubuntu@127.0.0.1:'+target],check=True,timeout=60)

nodes={node.name:node for node in ast.walk(ast.parse(source.read_text())) if isinstance(node,ast.FunctionDef)}
exec(compile(ast.Module(body=[nodes['capture_guest'],nodes['graphical_tests']],type_ignores=[]),str(source),'exec'),globals())
with (EVIDENCE/'qemu.log').open('w') as qemu_log:
    vm=subprocess.Popen(command,env=env,stdout=qemu_log,stderr=qemu_log)
    result['qemu_pid']=vm.pid
    try:
        deadline=time.monotonic()+100
        while time.monotonic()<deadline:
            assert vm.poll() is None,'private QEMU exited before SSH readiness'
            try:
                if subprocess.run([*ssh_base,'true'],capture_output=True,timeout=5).returncode==0:break
            except subprocess.TimeoutExpired:pass
            time.sleep(.5)
        else:raise RuntimeError('debug guest SSH did not become ready')
        assert ssh('systemd-detect-virt --vm').stdout.strip()=='kvm'
        payload=ssh('sha256sum /usr/bin/omen-gui /usr/bin/omen-daemon /usr/bin/omen-cli').stdout
        result['installed_payload']=payload.splitlines()
        graphical_tests()
        data=json.loads(ssh('sudo -n cat /var/tmp/omen-vm-result.json').stdout)
        result['graphical_authorization']=data['graphical_authorization']
        assert result['graphical_authorization']['result']=='passed'
        result['result']='passed'
    except BaseException as error:
        result['error']=type(error).__name__+': '+str(error)
        raise
    finally:
        if vm.poll() is None:
            try:
                partial=ssh('sudo -n cat /var/tmp/omen-vm-result.json',check=False)
                if partial.returncode==0:result['guest_result']=json.loads(partial.stdout)
                for name,command_text in [('desktop','sudo -n journalctl --no-pager -u sddm.service -u user@1000.service'),('polkit','sudo -n journalctl --no-pager -u polkit.service')]:
                    (EVIDENCE/(name+'-journal.log')).write_text(ssh(command_text,check=False).stdout)
                ssh('sudo -n systemctl poweroff',check=False)
                vm.wait(timeout=30)
            except (subprocess.SubprocessError,OSError):
                vm.terminate()
                try:vm.wait(timeout=10)
                except subprocess.TimeoutExpired:vm.kill();vm.wait()
        result['vm_exited']=vm.poll() is not None;result['vm_exit_code']=vm.returncode
        result['private_inputs_retained_for_debug']=work.exists()
        (EVIDENCE/'result.json').write_text(json.dumps(result,indent=2)+'\n')
        log.close()
print(json.dumps({'result':result['result'],'evidence':str(EVIDENCE)}))
