from pathlib import Path
import json, os, subprocess, sys
root = Path(__file__).resolve().parents[2]
base = Path(__file__).resolve().parent
out = base / sys.argv[1]
out.mkdir(exist_ok=False)
args = [sys.executable, str(root / 'tests/systemd_vm.py'), str(root / 'target/debian/omen-linux_0.1.0-20_amd64.deb'),
    '--offline-repository', str(base / 'complete/complete-repository.tar'),
    '--repository-component', 'main', '--repository-component', 'universe',
    '--memory-mib', '2048', '--graphical-authorization', '--keep-failed',
    '--managed-game-helper', str(root / 'target/release/deps/managed_games_native-50cfcaebb87f067b')]
for package in json.loads((base / 'requested-desktop-packages.json').read_text()):
    args += ['--extra-package', package]
(out / 'command.json').write_text(json.dumps(args, indent=2) + '\n')
with (out / 'driver.log').open('w') as log:
    code = subprocess.run(args, env=dict(os.environ, OMEN_TEST_EVIDENCE_DIR=str(out)), stdout=log, stderr=subprocess.STDOUT).returncode
print(json.dumps({'evidence': str(out), 'exit_code': code}))
raise SystemExit(code)
