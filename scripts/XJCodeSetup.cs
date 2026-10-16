using System;
using System.IO;
using System.Reflection;
using System.Diagnostics;
using System.Drawing;
using System.Text;
using System.Threading.Tasks;
using System.Windows.Forms;
class XJCodeSetup : Form {
 TextBox log; Label status; ProgressBar progress; Button finish; bool completed;
 static string installRoot;
 public XJCodeSetup() {
  Text="XJCode · 安装"; Width=660; Height=460; StartPosition=FormStartPosition.CenterScreen;
  BackColor=Color.FromArgb(12,23,25); ForeColor=Color.FromArgb(230,239,236);
  Font=new Font("Microsoft YaHei",10); FormBorderStyle=FormBorderStyle.FixedDialog; MaximizeBox=false;
  var title=new Label{Text="◆  XJCode",Location=new Point(28,22),AutoSize=true,Font=new Font("Arial",24,FontStyle.Bold),ForeColor=Color.FromArgb(77,211,181)};
  status=new Label{Text="正在准备编程工作台，首次安装可能需要几分钟…",Location=new Point(30,78),AutoSize=true};
  log=new TextBox{Multiline=true,ReadOnly=true,ScrollBars=ScrollBars.Vertical,Location=new Point(30,115),Size=new Size(582,215),BackColor=Color.FromArgb(19,35,38),ForeColor=ForeColor,BorderStyle=BorderStyle.FixedSingle,Font=new Font("Microsoft YaHei",9)};
  progress=new ProgressBar{Location=new Point(30,345),Size=new Size(450,16),Style=ProgressBarStyle.Marquee};
  finish=new Button{Text="完成",Location=new Point(512,339),Size=new Size(100,34),Enabled=false,BackColor=Color.FromArgb(77,211,181),ForeColor=Color.FromArgb(12,23,25),FlatStyle=FlatStyle.Flat};
  finish.Click+=(s,e)=>Close();Controls.AddRange(new Control[]{title,status,log,progress,finish});
  FormClosing+=(s,e)=>{if(!completed)e.Cancel=true;};
  Shown+=(s,e)=>Task.Factory.StartNew(()=>{
   int result=Install(installRoot,false,Append);
   BeginInvoke(new Action(()=>{completed=true;progress.Style=ProgressBarStyle.Continuous;progress.Value=result==0?100:0;finish.Enabled=true;status.Text=result==0?"安装完成。新开 PowerShell，运行 xjcode login 即可开始。":"安装未完成，请查看下面的错误信息。";}));
  });
 }
 void Append(string text){if(!IsDisposed)BeginInvoke(new Action(()=>{log.AppendText(text+Environment.NewLine);}));}
 static string Quote(string text){if(text.Contains("\""))throw new ArgumentException("Invalid path");return "\""+text.TrimEnd('\\')+"\"";}
 static int Install(string root,bool noPath,Action<string> output){
  string temp=Path.Combine(Path.GetTempPath(),"xjcode-setup-"+Guid.NewGuid().ToString("N"));
  Directory.CreateDirectory(temp);
  try {
   var assembly=Assembly.GetExecutingAssembly();
   foreach(string resource in assembly.GetManifestResourceNames()){
    if(!resource.StartsWith("payload."))continue;
    string name=resource.Substring(8);
    if(Path.GetFileName(name)!=name)throw new Exception("Invalid bundled resource");
    using(var input=assembly.GetManifestResourceStream(resource))
    using(var target=File.Create(Path.Combine(temp,name)))input.CopyTo(target);
   }
   var info=new ProcessStartInfo();
   info.FileName=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System),"WindowsPowerShell","v1.0","powershell.exe");
   info.Arguments="-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "+Quote(Path.Combine(temp,"bootstrap-xjcode.ps1"))+" -BundleRoot "+Quote(temp);
   if(root!=null)info.Arguments+=" -InstallRoot "+Quote(Path.GetFullPath(root));
   if(noPath)info.Arguments+=" -NoPath";
   info.UseShellExecute=false;info.CreateNoWindow=true;info.RedirectStandardOutput=true;info.RedirectStandardError=true;
   info.StandardOutputEncoding=Encoding.UTF8;info.StandardErrorEncoding=Encoding.UTF8;
   using(var process=new Process()){
    process.StartInfo=info;
    process.OutputDataReceived+=(s,e)=>{if(e.Data!=null)output(e.Data);};
    process.ErrorDataReceived+=(s,e)=>{if(e.Data!=null)output(e.Data);};
    process.Start();process.BeginOutputReadLine();process.BeginErrorReadLine();process.WaitForExit();
    return process.ExitCode;
   }
  }catch(Exception error){output(error.Message);return 1;}
  finally{try{Directory.Delete(temp,true);}catch{}}
 }
 [STAThread] static int Main(string[] args){
  bool silent=Array.IndexOf(args,"--silent")>=0,noPath=Array.IndexOf(args,"--no-path")>=0;
  int index=Array.IndexOf(args,"--install-root");
  if(index>=0){if(index+1>=args.Length)return 2;installRoot=args[index+1];}
  if(silent){
   if(installRoot==null)return 2;
   var text=new StringBuilder();
   int result=Install(installRoot,noPath,line=>{lock(text){text.AppendLine(line);}});
   Directory.CreateDirectory(installRoot);File.WriteAllText(Path.Combine(installRoot,"setup-result.log"),text.ToString(),Encoding.UTF8);
   return result;
  }
  Application.EnableVisualStyles();Application.SetCompatibleTextRenderingDefault(false);Application.Run(new XJCodeSetup());return 0;
 }
}
