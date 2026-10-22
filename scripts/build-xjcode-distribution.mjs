// Build on Windows: node scripts/build-xjcode-distribution.mjs [native-zip]
import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {execFileSync} from 'node:child_process';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const packageDir=path.join(root,'npm/xjcode');
const pkg=JSON.parse(await fs.readFile(path.join(packageDir,'package.json'),'utf8'));
const node=JSON.parse(await fs.readFile(path.join(root,'scripts/node-runtime.json'),'utf8'));
const version=pkg.version;
const web=path.join(root,'dist/web'),release=path.join(web,'releases',version);
await fs.mkdir(release,{recursive:true});
const nativeZip=path.resolve(process.argv[2]||path.join(root,'dist', 'xjcode-'+version+'-windows-x64-dev.zip'));
const hash=async file=>crypto.createHash('sha256').update(await fs.readFile(file)).digest('hex');
const nativeHash=await hash(nativeZip);
await fs.writeFile(path.join(packageDir,'release.json'),JSON.stringify({
 version,nativeUrl:'https://xjcode.wenshanzhike.com/releases/'+version+'/xjcode-windows-x64.zip',nativeSha256:nativeHash
},null,2)+'\n');
const nativeDir=path.join(packageDir,'native');
try{
 const actual=await fs.realpath(nativeDir);
 if(actual.toLowerCase()!==nativeDir.toLowerCase())throw Error('Native build directory must not be a link.');
 await fs.rm(actual,{recursive:true,force:true});
}catch(e){if(e.code!=='ENOENT')throw e;}
const powershell=path.join(process.env.SystemRoot||'C:/Windows','System32/WindowsPowerShell/v1.0/powershell.exe');
execFileSync(powershell,['-NoProfile','-NonInteractive','-WindowStyle','Hidden','-ExecutionPolicy','Bypass','-File',path.join(root,'scripts/extract-xjcode.ps1')],{windowsHide:true,stdio:'pipe',env:{...process.env,XJCODE_EXTRACT_ZIP:nativeZip,XJCODE_EXTRACT_TO:nativeDir}});

const npmCli=process.env.npm_execpath||path.join(path.dirname(process.execPath),'node_modules/npm/bin/npm-cli.js');
execFileSync(process.execPath,[npmCli,'pack','--pack-destination',path.join(root,'dist')],{cwd:packageDir,windowsHide:true,stdio:'pipe'});
const tarball='xjcode-'+version+'.tgz';
await fs.copyFile(path.join(root,'dist',tarball),path.join(release,tarball));
await fs.copyFile(nativeZip,path.join(release,'xjcode-windows-x64.zip'));
await fs.copyFile(path.join(root,'scripts/bootstrap-xjcode.ps1'),path.join(release,'bootstrap-xjcode.ps1'));
const nodeFile=path.join(release,node.file);
try{await fs.access(nodeFile);}catch{
 const response=await fetch(node.url,{signal:AbortSignal.timeout(300000)});
 if(!response.ok)throw Error('Node download failed: '+response.status);
 const candidate=nodeFile+'.download';
 await fs.writeFile(candidate,Buffer.from(await response.arrayBuffer()));
 if(await hash(candidate)!==node.sha256)throw Error('Node archive checksum mismatch');
 await fs.rename(candidate,nodeFile);
}
if(await hash(nodeFile)!==node.sha256)throw Error('Cached Node checksum mismatch');
const manifest={version,npm:{file:tarball,sha256:await hash(path.join(release,tarball)),bundledNative:true},
 bootstrap:{file:'bootstrap-xjcode.ps1',sha256:await hash(path.join(release,'bootstrap-xjcode.ps1'))},
 native:{file:'xjcode-windows-x64.zip',sha256:nativeHash},node:{version:node.version,file:node.file,sha256:node.sha256}};
await fs.writeFile(path.join(release,'release-manifest.json'),JSON.stringify(manifest,null,2)+'\n');
const csc=path.join(process.env.SystemRoot||'C:/Windows','Microsoft.NET/Framework64/v4.0.30319/csc.exe');
const args=['/nologo','/target:winexe','/platform:x64','/codepage:65001',
 '/reference:System.Windows.Forms.dll','/reference:System.Drawing.dll',
 '/out:'+path.join(web,'XJCode-Setup.exe'),path.join(root,'scripts/XJCodeSetup.cs')];
for(const file of ['bootstrap-xjcode.ps1','release-manifest.json',tarball,node.file])
 args.push('/resource:'+path.join(release,file)+',payload.'+file);
execFileSync(csc,args,{cwd:root,windowsHide:true,stdio:'pipe'});
for(const file of ['index.html','style.css','app.js','favicon.svg'])
 await fs.copyFile(path.join(root,'site',file),path.join(web,file));
await fs.copyFile(path.join(root,'scripts/bootstrap-xjcode.ps1'),path.join(web,'install.ps1'));
const checksum=(await hash(path.join(web,'XJCode-Setup.exe')))+'  XJCode-Setup.exe\n';
await fs.writeFile(path.join(web,'SHA256SUMS.txt'),checksum);
await fs.writeFile(path.join(web,'latest.json'),JSON.stringify({schema:1,version,manifest_sha256:await hash(path.join(release,'release-manifest.json'))},null,2)+'\n');
console.log('Built npm tarball, offline setup executable and website: '+web);
