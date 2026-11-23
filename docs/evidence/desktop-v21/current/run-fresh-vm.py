from pathlib import Path
import json,os,subprocess
base=Path(__file__).resolve().parent
out=base/"fresh-vm"
out.mkdir(exist_ok=False)
with (out/"driver.log").open("w") as log:
 code=subprocess.run(json.loads((base/"fresh-vm-command.json").read_text()),env=dict(os.environ,OMEN_TEST_EVIDENCE_DIR=str(out)),stdout=log,stderr=subprocess.STDOUT).returncode
print(code)
raise SystemExit(code)
