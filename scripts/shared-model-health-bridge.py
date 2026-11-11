import contextlib,datetime,fcntl,hashlib,json,os,pathlib,re,sys,tempfile
P=pathlib.Path
BASE=P('/home/codex-admin/services/yun-website-origin/releases')
SOURCE=BASE/'teaching-main-static-20261031-bd527b/sites/main/model-health-20261111/status.json'
DEST=BASE/'xjcode-site-20261031-2bcc73/site/shared-model-status.json'
LOCKS=['/opt/fracture-proof/.deploy.lock','/opt/matterswarm-workstation/.deploy.lock']
def sync(source,dest):
 assert source.is_file() and not source.is_symlink() and dest.parent.is_dir() and not dest.parent.is_symlink()
 assert not dest.is_symlink()
 data=source.read_bytes();assert len(data)<65536
 report=json.loads(data);assert report.get('version')==1 and isinstance(report.get('models'),dict) and 0<len(report['models'])<=100
 stamp=datetime.datetime.fromisoformat(report['checked_at']);assert stamp.tzinfo is not None
 for name,item in report['models'].items():assert re.fullmatch(r'[A-Za-z0-9._-]{1,80}',name) and isinstance(item,dict) and item.get('state') in ('available','unknown','upstream_unavailable')
 if dest.is_file() and hashlib.sha256(dest.read_bytes()).digest()==hashlib.sha256(data).digest():return 'unchanged'
 fd,tmp=tempfile.mkstemp(prefix='.health-',dir=dest.parent)
 try:
  with os.fdopen(fd,'wb') as f:f.write(data);f.flush();os.fsync(f.fileno());os.fchmod(f.fileno(),0o644)
  os.replace(tmp,dest)
 finally:
  if os.path.exists(tmp):os.unlink(tmp)
 return 'updated'
def selftest(base):
 base.mkdir(parents=True,exist_ok=False);src=base/'source.json';dst=base/'status.json'
 data={'version':1,'checked_at':'2026-11-11T00:00:00+00:00','models':{'demo':{'state':'upstream_unavailable'}}};src.write_text(json.dumps(data));assert sync(src,dst)=='updated' and sync(src,dst)=='unchanged';before=dst.read_bytes()
 data['models']['demo']['state']='invalid';src.write_text(json.dumps(data))
 try:sync(src,dst);raise RuntimeError('bad status accepted')
 except AssertionError:pass
 assert dst.read_bytes()==before;print('bridge selftest passed')
def main():
 with contextlib.ExitStack() as st:
  for path in LOCKS:
   f=st.enter_context(open(path,'r'));fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)
  print(sync(SOURCE,DEST))
if __name__=='__main__':
 if len(sys.argv)>1 and sys.argv[1]=='--selftest':selftest(P(sys.argv[2]))
 else:main()
