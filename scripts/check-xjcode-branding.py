"""Windows UI regression check. Requires pywinpty and pyte. Never submits a model prompt."""
import os,sys,time,socket,pathlib,re,json,argparse
from winpty import PtyProcess
local_lib = pathlib.Path(__file__).resolve().parents[1] / '.build/terminal-preview-lib'
if local_lib.exists(): sys.path.insert(0,str(local_lib))
import pyte
parser=argparse.ArgumentParser()
parser.add_argument('exe')
parser.add_argument('--session-id')
parser.add_argument('--cwd',default=str(pathlib.Path.cwd()))
parser.add_argument('--output',default='.build/branding-check')
options=parser.parse_args()
exe=options.exe;mode='resume' if options.session_id else 'fresh';out=options.output
args=[exe,'-C',options.cwd]
if mode=='resume': args=[exe,'resume',options.session_id,'--no-alt-screen','-C',options.cwd]
env=dict(os.environ);env.update(TERM='xterm-256color',COLORTERM='truecolor',PATHEXT='.COM;.EXE;.BAT;.CMD',PYTHONIOENCODING='utf-8')
p=PtyProcess.spawn(args,env=env,dimensions=(48,100),backend=0)
p.fileobj.settimeout(.2)
screen=pyte.Screen(100,48);stream=pyte.Stream(screen);raw=[]
def pump(seconds):
    end=time.time()+seconds
    while time.time()<end:
        try: data=p.read(65536)
        except socket.timeout: continue
        except EOFError: return
        raw.append(data);stream.feed(data)
        if '\x1b[6n' in data:p.write('\x1b[1;1R')
        if '\x1b[c' in data:p.write('\x1b[?1;2c')
        if '\x1b]10;?' in data:p.write('\x1b]10;rgb:d2d2/dddd/ebeb\x1b\\')
        if '\x1b]11;?' in data:p.write('\x1b]11;rgb:0c0c/0c0c/0c0c\x1b\\')
pump(3)
for _ in range(20):
    if "开始创作" in "".join(raw) and (mode=="resume" or any(0x2800 < ord(c) < 0x2900 for c in "".join(raw))): break
    pump(2)
prefix=pathlib.Path(out)
prefix.parent.mkdir(parents=True,exist_ok=True)
prefix.with_suffix('.screen.txt').write_text('\n'.join(screen.display),encoding='utf-8')
prefix.with_suffix('.cells.json').write_text(json.dumps([[{'text':screen.buffer[y][x].data,'fg':screen.buffer[y][x].fg,'bold':screen.buffer[y][x].bold} for x in range(100)] for y in range(48)]),encoding='utf-8')
p.write('/quit');pump(.3);p.write('\r');pump(4)
text=''.join(raw)
prefix.with_suffix('.raw.txt').write_text(text,encoding='utf-8')
clean=re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]','',text)
checks={'old_resume_absent':not bool(re.search(r'(?<![a-z])codex resume',clean)),'new_resume_present':mode!='resume' or 'xjcode resume' in clean,'xjcode_header':'XJCode' in clean}
checks['old_header_absent']='OpenAI Codex' not in clean and 'Ask Codex' not in clean
checks['emblem_present']=mode=='resume' or sum(0x2800<ord(c)<0x2900 for c in clean)>100
print(json.dumps(checks),flush=True)
# pywinpty ConPTY finalization can hang after an already exited child.
import threading
closer=threading.Thread(target=lambda:p.close(force=True),daemon=True)
closer.start();closer.join(2)
os._exit(0 if all(checks.values()) else 1)
