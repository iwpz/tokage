from pathlib import Path
import subprocess,tempfile,sys,json
sys.path.insert(0,str(Path.cwd()/'scripts'))
from live_natural_fixture import ORACLE,FILES
root=Path.cwd()/'docs/reliability-evidence/m2g'
for label in ['opencode','claude']:
 folder=root/f'{label}-natural'
 if not (folder/'delivery.bundle').exists():continue
 proof=json.loads((folder/'proof.json').read_text())['proof']
 with tempfile.TemporaryDirectory(prefix='tokage-m2g-replay-') as d:
  repo=Path(d)/'repo'
  subprocess.run(['git','clone','--no-checkout',str(folder/'delivery.bundle'),str(repo)],check=True,capture_output=True)
  subprocess.run(['git','-C',str(repo),'-c','core.hooksPath=/dev/null','checkout','--detach',proof['after']],check=True,capture_output=True)
  files=subprocess.check_output(['git','-C',str(repo),'diff','--name-only',proof['before'],proof['after']],text=True).splitlines()
  if set(files)!=set(FILES):raise RuntimeError('Unexpected deliverables')
  baseline=repo/'baseline.patch'
  data=subprocess.check_output(['git','-C',str(repo),'diff','--binary',proof['before'],proof['after']])
  if data!=(folder/'delivery.patch').read_bytes():raise RuntimeError('Archived patch differs from actual commits')
  oracle=ORACLE
  for name in FILES:oracle=oracle.replace("'./"+name+"'",json.dumps((repo/name).as_uri()))
  target=Path(d)/'oracle.mjs';target.write_text(oracle)
  subprocess.run(['node',str(target),'all'],check=True)
  print('PASS',label,'bundle commit',proof['after'],'patch matches; independent original oracle passed',flush=True)
