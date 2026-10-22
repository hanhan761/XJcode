"""Isolated npm old->new staging and uninstall acceptance, no registry short package lookup."""
import argparse,hashlib,json,os,pathlib,subprocess,time
p=argparse.ArgumentParser();p.add_argument('--work-dir',required=True);a=p.parse_args()
repo=pathlib.Path(__file__).resolve().parents[1];work=pathlib.Path(a.work_dir).resolve();work.mkdir(parents=True,exist_ok=True)
root=work/'npm-prefix';home=work/'personal';root.mkdir();home.mkdir();(home/'keep.txt').write_text('keep')
node=pathlib.Path('D:/node/node.exe');npm=node.parent/'node_modules/npm/bin/npm-cli.js'
ps=pathlib.Path(os.environ['SystemRoot'])/'System32/WindowsPowerShell/v1.0/powershell.exe'
env={**os.environ,'XJCODE_HOME':str(home),'PSModulePath':os.environ['SystemRoot']+r'\System32\WindowsPowerShell\v1.0\Modules;C:\Program Files\WindowsPowerShell\Modules'}
def run(args):return subprocess.run([str(x) for x in args],env=env,capture_output=True,text=True,encoding='utf-8',errors='replace',creationflags=subprocess.CREATE_NO_WINDOW,timeout=180)
def ok(r):assert r.returncode==0,r.stdout[-2000:]+r.stderr[-2000:];return r
old=repo/'dist/web/releases/0.1.4/xjcode-0.1.4.tgz';new=repo/'dist/web/releases/0.1.5/xjcode-0.1.5.tgz'
ok(run([node,npm,'install','--global','--prefix',root,'--offline','--no-audit','--no-fund','--ignore-scripts',old]))
entry=root/'node_modules/xjcode/bin/xjcode.cjs';assert '0.1.4' in ok(run([node,entry,'--version'])).stdout
other=root/'node_modules/unrelated';other.mkdir();(other/'keep.txt').write_text('other-package')
job={'Action':'UpdateNpm','Kind':'npm','Root':str(root),'Node':str(node),'Npm':str(npm),'Package':str(new),'PackageHash':hashlib.file_digest(new.open('rb'),'sha256').hexdigest(),'Version':'0.1.5','WaitPids':[],'Gui':False}
file=work/'job.json';file.write_text(json.dumps(job));helper=repo/'scripts/manage-xjcode.ps1'
def update():return run([ps,'-NoProfile','-NonInteractive','-WindowStyle','Hidden','-ExecutionPolicy','Bypass','-File',helper,'-JobFile',file])
# A tampered hash must fail before replacing the installed version.
job['PackageHash']='0'*64;file.write_text(json.dumps(job));assert update().returncode!=0;assert '0.1.4' in ok(run([node,entry,'--version'])).stdout
job['PackageHash']=hashlib.file_digest(new.open('rb'),'sha256').hexdigest();file.write_text(json.dumps(job));ok(update())
assert ok(run([node,entry,'--version'])).stdout.strip()=='xjcode 0.1.5'
assert (root/'.xjcode-rollback/.xjcode-owned-rollback').read_text()=='XJCode rollback v1'
assert json.loads((root/'.xjcode-rollback/package.json').read_text(encoding='utf-8'))['version']=='0.1.4'
assert (other/'keep.txt').read_text()=='other-package'
r=ok(run([node,entry,'uninstall','--yes']))
limit=time.monotonic()+90
while (root/'node_modules/xjcode').exists() and time.monotonic()<limit:time.sleep(.25)
assert not (root/'node_modules/xjcode').exists(),r.stdout
assert not (root/'.xjcode-rollback').exists()
assert (other/'keep.txt').read_text()=='other-package' and (home/'keep.txt').read_text()=='keep'
result={'success':True,'checks':['npm-0.1.4-to-0.1.5','tampered-package-refused','staging-verifies-version','one-rollback-retained','npm-self-uninstall','unrelated-package-preserved','personal-data-preserved']}
(work/'npm-result.json').write_text(json.dumps(result,indent=2));print(json.dumps(result))
