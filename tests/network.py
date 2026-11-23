#!/usr/bin/env python3
"""Production network IPC/CLI with fixtures, an older daemon, and read-only host counters."""
import datetime
import hashlib
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
PROFILE = os.environ.get('OMEN_TEST_PROFILE', 'debug')
BIN = ROOT/'target'/PROFILE


def request(endpoint, op):
    assert op in ['get_info', 'get_network', 'get_capabilities', 'export_diagnostics']
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(5);stream.connect(endpoint)
        stream.sendall(json.dumps({'v':1,'id':1,'op':{'op':op}}).encode()+b'\n')
        return json.loads(stream.makefile('rb').readline())


def call(endpoint, op):
    value=request(endpoint,op);assert value['ok'],value
    return value['data']


def main():
    assert os.geteuid()!=0
    result={'result':'failed','profile':PROFILE,'hardware_control_writes':False,'host_network_policy_changed':False,
            'binary_sha256':{name:hashlib.sha256((BIN/name).read_bytes()).hexdigest() for name in ['omen-cli','omen-daemon']},
            'checks_passed':[]}
    children=[];logs=[]
    with tempfile.TemporaryDirectory(prefix='omen-network-',dir=ROOT/'target') as directory:
        temp=Path(directory)
        def start(name, binary, mock):
            area=temp/name;area.mkdir();(area/'tmp').mkdir()
            env=dict(os.environ,OMEN_CONFIG_DIR=str(area/'user'),OMEN_SYSTEM_CONFIG_DIR=str(area/'system'),
                     OMEN_SOCK=str(area/'control.sock'),TMPDIR=str(area/'tmp'))
            log=(area/'daemon.log').open('w');logs.append(log)
            child=subprocess.Popen([str(binary)]+(['--mock'] if mock else []),env=env,stdout=subprocess.PIPE,stderr=log,text=True)
            children.append(child)
            if mock:
                endpoint=child.stdout.readline().strip()
            else:
                endpoint=env['OMEN_SOCK'];limit=time.monotonic()+8
                while not Path(endpoint).exists():
                    assert child.poll() is None and time.monotonic()<limit,'read-only daemon did not start'
                    time.sleep(.02)
            assert call(endpoint,'get_info')['mock'] is mock
            return child,endpoint,env,area
        def passed(label):result['checks_passed'].append(label);print('PASS',label,flush=True)
        try:
            fixture,endpoint,env,area=start('fixture',BIN/'omen-daemon',True)
            report=call(endpoint,'get_network');assert report['source']=='fixture' and report['interval_ms'] is None
            assert len(report['interfaces'])==1 and report['interfaces'][0]['name']=='fixture0'
            assert report['interfaces'][0]['rx_bytes_per_second'] is None
            passed('first fixture acquisition reports counters and an unknown rate without reading host interfaces')
            fixture_root=next(p.parent.parent for p in (area/'tmp').glob('*/sys/class') if (p/'net/fixture0').is_dir())
            # The path above is the fixture root; only this child's private TMPDIR is searched.
            proc=fixture_root/'proc/net/dev';assert proc.is_file(),fixture_root
            def counters(rx,tx):
                proc.write_text(f'Inter-| Receive | Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\nfixture0: {rx} 100 2 3 0 0 0 0 {tx} 50 4 5 0 0 0 0\n')
            counters(1048576+2048,524288+1024);time.sleep(.3)
            report=call(endpoint,'get_network');interface=report['interfaces'][0]
            assert abs(interface['rx_bytes_per_second']/interface['tx_bytes_per_second']-2)<1e-9
            elapsed=2048/interface['rx_bytes_per_second']*1000
            assert report['interval_ms']<=elapsed<report['interval_ms']+1
            assert (interface['rx_errors'],interface['tx_errors'],interface['rx_dropped_or_missed'],interface['tx_dropped'])==(2,4,3,5)
            passed('production IPC rates match byte deltas and actual interval; errors/drops preserve kernel counter semantics')
            counters(1,524288+1024);time.sleep(.3);report=call(endpoint,'get_network')
            assert report['interfaces'][0]['rx_bytes_per_second'] is None and report['interfaces'][0]['tx_bytes_per_second']==0
            passed('counter decrease rebases that direction while unchanged traffic is a measured zero')
            (fixture_root/'sys/class/net/fixture0/ifindex').write_text('9')
            counters(999999,999999);time.sleep(.3)
            assert call(endpoint,'get_network')['interfaces'][0]['rx_bytes_per_second'] is None
            passed('an interface identity change suppresses the first rate instead of spiking')
            proc.unlink();time.sleep(.3);failure=request(endpoint,'get_network');assert not failure['ok']
            counters(1000000,1000000)
            restored=call(endpoint,'get_network');assert restored['interval_ms'] is None
            passed('missing counters fail explicitly and recovery starts a fresh baseline')
            command=[str(BIN/'omen-cli'),'--socket',endpoint,'network','--json','--interval-ms','250']
            cli=subprocess.run(command,env=env,capture_output=True,text=True,check=True,timeout=10)
            report=json.loads(cli.stdout);assert report['source']=='fixture' and report['interfaces'][0]['rx_bytes_per_second']==0
            assert report['interfaces'][0]['tx_bytes_per_second']==0
            diagnostics=call(endpoint,'export_diagnostics');assert diagnostics['network']['source']=='fixture' and diagnostics['network_error'] is None
            passed('CLI two-sample JSON and diagnostics use the same fixture-only production path')
            invalid=subprocess.run(command[:-1]+['1'],env=env,capture_output=True,text=True,timeout=5)
            assert invalid.returncode!=0
            passed('CLI rejects polling intervals below the supported bound')
            baseline=ROOT/'target/debian/omen-linux_0.1.0-6_amd64.deb';assert baseline.is_file()
            archive=subprocess.check_output(['dpkg-deb','--fsys-tarfile',str(baseline)])
            with tarfile.open(fileobj=io.BytesIO(archive)) as payload:
                data=payload.extractfile('./usr/bin/omen-daemon').read()
            old=temp/'old-daemon';old.write_bytes(data);old.chmod(0o700)
            old_child,old_endpoint,old_env,_=start('old',old,True)
            old_cli=subprocess.run([str(BIN/'omen-cli'),'--socket',old_endpoint,'network','--json','--interval-ms','250'],env=old_env,capture_output=True,text=True,timeout=10)
            assert old_cli.returncode!=0 and not old_cli.stdout.strip()
            result['old_daemon_sha256']=hashlib.sha256(data).hexdigest()
            passed('a revision 6 daemon reports the new operation unsupported; the client never substitutes host network data')
            real,live_endpoint,live_env,live_area=start('readonly',BIN/'omen-daemon',False)
            caps=call(live_endpoint,'get_capabilities');assert caps['performance_profiles']['status']!='available'
            first=call(live_endpoint,'get_network');assert first['source']=='kernel';time.sleep(.3)
            def independent():
                values={}
                for name in Path('/sys/class/net').iterdir():
                    try:values[name.name]={'ifindex':int((name/'ifindex').read_text()),'rx_bytes':int((name/'statistics/rx_bytes').read_text()),'tx_bytes':int((name/'statistics/tx_bytes').read_text())}
                    except (OSError,ValueError):pass
                return values
            before=independent();second=call(live_endpoint,'get_network');after=independent()
            verified=[]
            for interface in second['interfaces']:
                name=interface['name']
                if name not in before or name not in after:continue
                if before[name]['ifindex']!=after[name]['ifindex'] or before[name]['ifindex']!=interface['ifindex']:continue
                for counter in ['rx_bytes','tx_bytes']:
                    assert before[name][counter]<=interface[counter]<=after[name][counter],(name,counter,before[name],interface,after[name])
                assert interface['rx_bytes_per_second'] is not None and interface['tx_bytes_per_second'] is not None
                verified.append(name)
            assert verified
            result['live_readonly']={'kernel':os.uname().release,'uid':os.geteuid(),'first':first,'second':second,
                                     'independent_sysfs_before':before,'independent_sysfs_after':after,'verified_interfaces':verified}
            passed('unprivileged real daemon counters fall between independent sysfs reads; no traffic is generated and controls remain disabled')
            assert "op 'set_" not in (live_area/'daemon.log').read_text()
            result['result']='passed'
        except BaseException as error:
            result['error']=str(error);raise
        finally:
            for child in reversed(children):
                if child.poll() is None:child.terminate();assert child.wait(timeout=8)==0
            for log in logs:log.close()
            for path in temp.glob('*/daemon.log'):
                (ROOT/'docs/evidence'/f'network-{PROFILE}-{path.parent.name}-daemon.log').write_text(path.read_text())
            result['cleanup_complete']=True;result['recorded_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
            (ROOT/'docs/evidence'/f'network-{PROFILE}.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))


if __name__=='__main__':main()
