"""Windows isolated lifecycle acceptance. Never touches the real XJCode home or user PATH."""
import argparse, hashlib, json, os, pathlib, subprocess, time, winreg
p=argparse.ArgumentParser();p.add_argument('--work-dir',required=True);a=p.parse_args()
repo=pathlib.Path(__file__).resolve().parents[1]
work=pathlib.Path(a.work_dir).resolve();work.mkdir(parents=True,exist_ok=True)
root=work/'isolated install 中文';home=work/'personal-data'
assert not root.exists(), 'Use a fresh isolated test directory'
web=repo/'dist/web';exe=web/'XJCode-Setup.exe'
manifest=json.loads((web/'releases/0.1.5/release-manifest.json').read_text())
ps=pathlib.Path(os.environ['SystemRoot'])/'System32/WindowsPowerShell/v1.0/powershell.exe'
env={**os.environ,'PSModulePath':os.environ['SystemRoot']+r'\System32\WindowsPowerShell\v1.0\Modules;C:\Program Files\WindowsPowerShell\Modules','XJCODE_HOME':str(home)}
def run(argv,**kw):
 r=subprocess.run([str(v) for v in argv],env=kw.pop('env',env),capture_output=True,text=True,encoding='utf-8',errors='replace',creationflags=subprocess.CREATE_NO_WINDOW,timeout=kw.pop('timeout',180),**kw)
 return r
def expect(r):
 assert r.returncode==0, r.stdout[-3000:]+r.stderr[-3000:]
 return r
with winreg.OpenKey(winreg.HKEY_CURRENT_USER,'Environment') as k:before_path=winreg.QueryValueEx(k,'Path')[0]
no_node={**env,'PATH':str(ps.parent)+';'+os.environ['SystemRoot']+r'\System32','PATHEXT':'.COM;.EXE;.BAT;.CMD'}
r=run([exe,'--silent','--no-path','--install-root',root],env=no_node,timeout=300)
if r.returncode:raise AssertionError((root/'setup-result.log').read_text(encoding='utf-8-sig'))
state=json.loads((root/'installation.json').read_text(encoding='utf-8-sig'))
assert state['Product']=='XJCode' and state['Version']=='0.1.5' and state['PrivateNode'] and not state['PathAdded']
assert (root/'XJCode-Uninstall.exe').is_file()
with winreg.OpenKey(winreg.HKEY_CURRENT_USER,chr(92).join(('Software','Microsoft','Windows','CurrentVersion','Uninstall',state['RegistryKey']))) as k:assert winreg.QueryValueEx(k,'DisplayVersion')[0]=='0.1.5'
entry=next((root/'apps').glob('*/node_modules/xjcode/bin/xjcode.cjs'));node=pathlib.Path(state['Node'])
assert expect(run([node,entry,'--version'])).stdout.strip()=='xjcode 0.1.5'
expect(run([exe,'--silent','--no-path','--install-root',root],env=no_node,timeout=300))
# Failure before activation must leave the installed entry working.
bad=work/'bad-bundle';bad.mkdir();(bad/'release-manifest.json').write_text(json.dumps(manifest));(bad/manifest['npm']['file']).write_bytes(b'corrupt test package')
r=run([ps,'-NoProfile','-NonInteractive','-WindowStyle','Hidden','-ExecutionPolicy','Bypass','-File',repo/'scripts/bootstrap-xjcode.ps1','-BundleRoot',bad,'-InstallRoot',root,'-NoPath'])
assert r.returncode!=0
assert expect(run([node,entry,'--version'])).stdout.strip()=='xjcode 0.1.5'
# Existing processes cause a clean refusal before deletion.
busy=subprocess.Popen([str(node),'-e','setTimeout(()=>{},60000)'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=subprocess.CREATE_NO_WINDOW)
def maintenance(target,**options):
 job={'Action':'Uninstall','Kind':'managed','Root':str(target),'WaitPids':[],'Purge':False,'Home':str(home),'Gui':False,**options}
 file=work/'maintenance-test-job.json';file.write_text(json.dumps(job))
 return run([ps,'-NoProfile','-NonInteractive','-WindowStyle','Hidden','-ExecutionPolicy','Bypass','-File',repo/'scripts/manage-xjcode.ps1','-JobFile',file])
try:
 r=maintenance(root);assert r.returncode!=0 and (root/'installation.json').exists()
finally:busy.terminate();busy.wait()
# Ownership entries cannot escape or traverse junctions.
outside=work/'outside';outside.mkdir();(outside/'keep.txt').write_text('keep')
fake=work/'fake';fake.mkdir();record={'Schema':1,'Product':'XJCode','Root':str(fake),'RegistryKey':'XJCode-aaaaaaaaaaaa','Files':['../outside/keep.txt']}
(fake/'installation.json').write_text(json.dumps(record));assert maintenance(fake).returncode!=0;assert (outside/'keep.txt').read_text()=='keep'
def q(v):return "'"+str(v).replace("'","''")+"'"
link=fake/'linked';expect(run([ps,'-NoProfile','-NonInteractive','-WindowStyle','Hidden','-Command','New-Item -ItemType Junction -Path '+q(link)+' -Target '+q(outside)+' | Out-Null']))
record['Files']=['linked/keep.txt'];(fake/'installation.json').write_text(json.dumps(record));assert maintenance(fake).returncode!=0;assert (outside/'keep.txt').read_text()=='keep'
assert os.path.realpath(link)==str(outside);os.rmdir(link)
# CLI self-removal waits for its private Node launcher and preserves personal/unowned files.
home.mkdir(exist_ok=True);(home/'.xjcode-data-owner').write_text('XJCode data v1');(home/'session.txt').write_text('keep-session')
(root/'user-note.txt').write_text('unowned-user-file')
r=expect(run([node,entry,'uninstall','--yes']))
limit=time.monotonic()+90
while (root/'installation.json').exists() and time.monotonic()<limit:time.sleep(.25)
assert not (root/'installation.json').exists(),r.stdout
assert not node.exists() and not (root/'commands/xjcode.cmd').exists()
assert (home/'session.txt').read_text()=='keep-session' and (root/'user-note.txt').read_text()=='unowned-user-file'
try:winreg.OpenKey(winreg.HKEY_CURRENT_USER,chr(92).join(('Software','Microsoft','Windows','CurrentVersion','Uninstall',state['RegistryKey'])));raise AssertionError('Uninstall registration remains')
except FileNotFoundError:pass
# Reinstall and exercise explicitly requested purge against only the marked fixture home.
expect(run([exe,'--silent','--no-path','--install-root',root],env=no_node,timeout=300))
state=json.loads((root/'installation.json').read_text(encoding='utf-8-sig'));node=pathlib.Path(state['Node']);entry=next((root/'apps').glob('*/node_modules/xjcode/bin/xjcode.cjs'))
r=expect(run([node,entry,'uninstall','--purge','--yes']))
limit=time.monotonic()+90
while ((root/'installation.json').exists() or home.exists()) and time.monotonic()<limit:time.sleep(.25)
assert not home.exists() and not (root/'installation.json').exists(),r.stdout
with winreg.OpenKey(winreg.HKEY_CURRENT_USER,'Environment') as k:assert winreg.QueryValueEx(k,'Path')[0]==before_path
result={'success':True,'version':'0.1.5','checks':['offline-no-node-install','custom-Chinese-path','idempotent-install','Windows-uninstall-registration','corrupt-artifact-preserves-original','active-process-refusal','ownership-traversal-refusal','junction-refusal','CLI-private-node-self-removal','default-preserves-data','unowned-files-preserved','explicit-purge','user-PATH-unchanged']}
(work/'lifecycle-result.json').write_text(json.dumps(result,indent=2));print(json.dumps(result))
