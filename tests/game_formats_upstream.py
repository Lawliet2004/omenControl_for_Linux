"""Exercise unmodified Legendary on synthetic, offline-only private records."""
import hashlib
import json
import os
from pathlib import Path
import shlex
import sys
import time

ROOT = Path(__file__).resolve().parents[1]


def verify_upstream(temp, env, bins, config, default, selected, game, source, dependencies,
                    endpoint, launch, call, start, passed, result, binaries):
    provenance = json.loads((ROOT / 'docs/evidence/launcher-expansion/launcher-sources.json').read_text())
    pinned = next(repo for repo in provenance if repo['repository'] == 'legendary-gl/legendary')
    for entry in pinned['files']:
        assert hashlib.file_digest((source / entry['path']).open('rb'), 'sha256').hexdigest() == entry['sha256']
    marker = temp / 'actual-game.json'
    attempts = temp / 'network-attempts'
    module_log = temp / 'legendary-runtime.jsonl'
    guard = temp / 'offline-guard'
    guard.mkdir()
    (guard / 'sitecustomize.py').write_text(
        'import os,socket\n'
        'def refuse(*args,**kwargs):\n'
        ' with open(os.environ["OMEN_NETWORK_ATTEMPTS"],"a") as f: f.write("outbound attempt\\n")\n'
        ' raise RuntimeError("Offline fixture forbids network access")\n'
        'socket.socket.connect=refuse\nsocket.create_connection=refuse\nsocket.getaddrinfo=refuse\n')
    (game / 'game.py').write_text('#!/usr/bin/python3\nimport json,os,sys,time\nfrom pathlib import Path\n'
        'Path(os.environ["OMEN_FIXTURE_MARKER"]).write_text(json.dumps({"pid":os.getpid(),"uid":os.getuid(),'
        '"cwd":os.getcwd(),"args":sys.argv[1:],"request_env":os.environ.get("OMEN_LAUNCH_REQUEST_ID")}))\n'
        'time.sleep(float(os.environ.get("OMEN_GAME_SECONDS","0.3")))\n')
    (game / 'game.py').chmod(0o700)
    (bins / 'legendary').write_text('#!/usr/bin/python3\nimport hashlib,json,os,sys\nfrom pathlib import Path\n'
        'from legendary import cli\n'
        'with open(os.environ["OMEN_LEGENDARY_MODULE_LOG"],"a") as f: f.write(json.dumps({'
        '"module":cli.__file__,"sha256":hashlib.sha256(Path(cli.__file__).read_bytes()).hexdigest(),'
        '"args":sys.argv[1:],"configuration":os.environ["LEGENDARY_CONFIG_PATH"]})+"\\n")\n'
        'cli.main()\n')
    upstream_env = dict(env, PATH=str(bins) + os.pathsep + os.defpath,
        PYTHONPATH=os.pathsep.join(map(str, [guard, source, dependencies])),
        OMEN_FIXTURE_MARKER=str(marker), OMEN_NETWORK_ATTEMPTS=str(attempts),
        OMEN_LEGENDARY_MODULE_LOG=str(module_log), OMEN_GAME_SECONDS='0.3')
    for identifier, root in [('legendary/--literal', default), ('legendary/Other', selected)]:
        marker.unlink(missing_ok=True)
        call(endpoint, 'set_profile', {'profile': 'balanced'})
        receipt = launch(identifier, endpoint, upstream_env)
        deadline = time.monotonic() + 10
        while not marker.exists() and time.monotonic() < deadline:
            time.sleep(.04)
        assert marker.exists(), 'upstream CLI did not run the synthetic game'
        actual = json.loads(marker.read_text())
        assert actual['uid'] == os.getuid() and actual['cwd'] == str(game)
        assert '-epicapp=' + identifier.split('/', 1)[1] in actual['args']
        assert actual['request_env'] is None
        loaded = json.loads(module_log.read_text().splitlines()[-1])
        assert loaded['configuration'] == str(root)
        assert not receipt['launch']['launcher']['game_execution_verified']
        passed(identifier + ': pinned upstream Legendary executes the synthetic offline game from its selected configuration')
    assert not attempts.exists()
    helper = Path(os.environ['OMEN_NATIVE_GAME_TEST_BIN']).resolve()
    bus = start(['dbus-daemon', '--session', '--nofork', '--print-address=1'], 'private-ppd-bus', upstream_env)
    address = bus.stdout.readline().strip()
    assert address.startswith('unix:')
    managed_env = dict(upstream_env, DBUS_SYSTEM_BUS_ADDRESS=address, OMEN_GAME_SECONDS='3')
    ppd = start([str(helper), 'ppd-service', address], 'private-ppd', managed_env)
    assert ppd.stdout.readline().strip() == 'READY'
    endpoint = temp / 'managed.sock'
    service = start([str(helper), 'fixture-service', str(endpoint), str(temp / 'sessions.json'), address, 'mock'],
                    'production-service', managed_env)
    assert service.stdout.readline().strip() == 'READY'
    assert call(endpoint, 'get_info')['mock']
    call(endpoint, 'set_profile', {'profile': 'balanced'})
    config.write_text(config.read_text().replace('[games.per_game_profiles]', '[games]\nmanaged_game_ids=["legendary/Fixture"]\n[games.per_game_profiles]'))
    prefix = shlex.join([str(binaries / 'omen-cli'), 'game-run', 'legendary/Fixture', '--'])
    ini = default / 'config.ini'
    ini.write_text(ini.read_text().replace('[Fixture]', '[Fixture]\nwrapper = ' + prefix))
    managed_env['OMEN_SOCK'] = str(endpoint)
    marker.unlink(missing_ok=True)
    receipt = launch('legendary/Fixture', endpoint, managed_env)
    assert receipt['launch']['managed'] and not receipt['launch']['profile_was_applied']
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        sessions = call(endpoint, 'get_game_sessions')['sessions']
        if marker.exists() and any(s['profile_held'] for s in sessions):
            break
        time.sleep(.04)
    else:
        raise RuntimeError('actual Legendary wrapper did not register a live game hold')
    actual = json.loads(marker.read_text())
    assert actual['uid'] == os.getuid() and actual['request_env'] is None
    assert call(endpoint, 'get_profile') == 'performance'
    result['managed_live_sessions'] = sessions
    passed('upstream Legendary applies its configured actual-command wrapper and holds the fixture profile while the game is alive')
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        sessions = call(endpoint, 'get_game_sessions')['sessions']
        if sessions and all(s['phase'] == 'ended' and not s['profile_held'] for s in sessions):
            break
        time.sleep(.04)
    else:
        raise RuntimeError('actual Legendary game exit did not release its session')
    assert call(endpoint, 'get_profile') == 'balanced'
    assert not attempts.exists(), 'the offline upstream runtime attempted network access'
    result['upstream_legendary_tested'] = True
    result['upstream_source_commit'] = pinned['commit']
    result['upstream_runtime_invocations'] = [json.loads(line) for line in module_log.read_text().splitlines()]
    result['synthetic_game'] = actual
    result['managed_ended_sessions'] = sessions
    result['upstream_network_attempts'] = 0
    result['outbound_network_guard'] = 'process-local socket/DNS rejection; no upstream source modifications'
    result['managed_helper_sha256'] = hashlib.file_digest(helper.open('rb'), 'sha256').hexdigest()
    passed('the real synthetic game exit releases only its owned private-PPD hold; offline execution makes no network attempt')
