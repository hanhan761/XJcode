using System;
using System.IO;
using System.Diagnostics;
using System.Reflection;
class XJCodeUninstall {
 [STAThread] static int Main() {
  string root=Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);
  string ps=Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System),"WindowsPowerShell","v1.0","powershell.exe");
  var info=new ProcessStartInfo(ps,"-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File \""+Path.Combine(root,"maintenance","manage-xjcode.ps1")+"\" -Root \""+root+"\" -WaitPid "+Process.GetCurrentProcess().Id+" -Gui");
  info.UseShellExecute=false;info.CreateNoWindow=true;
  Process.Start(info);return 0;
 }
}
