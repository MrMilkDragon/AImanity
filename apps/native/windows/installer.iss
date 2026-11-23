[Setup]
AppId=dev.aimanity.workspace
AppName=AImanity
AppMutex=Local\dev.aimanity.workspace.running
AppVersion={#AppVersion}
VersionInfoVersion={#NumericVersion}
DefaultDirName={autopf}\AImanity Workspace
DefaultGroupName=AImanity Workspace
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
SetupArchitecture=x64
MinVersion=10.0.22000
PrivilegesRequired=admin
Compression=lzma2/ultra64
LZMANumBlockThreads=2
SolidCompression=yes
WizardStyle=modern
DisableProgramGroupPage=yes
UninstallFilesDir={app}
UninstallDisplayIcon={app}\ai-phone-client.exe
SetupIconFile={#ProjectRoot}\apps\native\icons\icon.ico
LicenseFile={#ProjectRoot}\LICENSE

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinese"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"
Name: "traditionalchinese"; MessagesFile: "compiler:Languages\ChineseTraditional.isl"
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "korean"; MessagesFile: "compiler:Languages\Korean.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "arabic"; MessagesFile: "compiler:Languages\Arabic.isl"

[Files]
Source: "{#AppSource}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\AImanity"; Filename: "{app}\ai-phone-client.exe"; AppUserModelID: "dev.aimanity.workspace"

[Run]
Filename: "{app}\ai-phone-client.exe"; Description: "{cm:LaunchProgram,AImanity}"; Flags: nowait postinstall skipifsilent

[Messages]
english.ConfirmUninstall=Remove %1 and delete its devices, chats and settings?
chinese.ConfirmUninstall=卸载 %1 并删除设备、聊天和设置？
traditionalchinese.ConfirmUninstall=解除安裝 %1 並刪除裝置、聊天與設定？
japanese.ConfirmUninstall=%1 をアンインストールし、デバイス、チャット、設定を削除しますか？
korean.ConfirmUninstall=%1을 제거하고 기기, 대화 및 설정을 삭제할까요?
german.ConfirmUninstall=%1 deinstallieren und Geräte, Chats und Einstellungen löschen?
french.ConfirmUninstall=Désinstaller %1 et supprimer ses appareils, conversations et réglages ?
spanish.ConfirmUninstall=¿Desinstalar %1 y eliminar los dispositivos, chats y ajustes?
russian.ConfirmUninstall=Удалить %1 вместе с устройствами, чатами и настройками?
arabic.ConfirmUninstall=هل تريد إزالة %1 وحذف أجهزته ومحادثاته وإعداداته؟

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  ResultCode: Integer;
  Output: TExecOutput;
begin
  if CurUninstallStep = usUninstall then
  begin
    if not ExecAndCaptureOutput(ExpandConstant('{app}\ai-phone-client.exe'),
      '--uninstall-windows-user-data', '', SW_HIDE, ewWaitUntilTerminated, ResultCode, Output) then
      RaiseException(SysErrorMessage(ResultCode));
    if ResultCode <> 0 then
      RaiseException('AImanity (' + IntToStr(ResultCode) + ')' + #13#10 + StringJoin(#13#10, Output.StdOut));
    Log('Removed AImanity local profile data.');
  end;
end;
