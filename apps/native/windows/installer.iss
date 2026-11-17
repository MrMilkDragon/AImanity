#define AluminumBaseSHA256 "348baa7207524397b74de603f08357b6d3b73947514eb9f662c6eb4e52a365a5"

[Setup]
AppId=dev.aiphone.client
AppName=AImanity
AppMutex=Local\dev.aiphone.client.running
AppVersion={#AppVersion}
VersionInfoVersion={#NumericVersion}
DefaultDirName={autopf}\AImanity
DefaultGroupName=AImanity
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
Source: "{#AppSource}\ai-phone-client.exe"; DestDir: "{tmp}"; Flags: dontcopy noencryption
Source: "{#AppSource}\host\qemu\guest\guest.qcow2"; DestDir: "{app}\host\qemu\guest"; Flags: onlyifdoesntexist
Source: "{#AppSource}\*"; DestDir: "{app}"; Excludes: "\host\qemu\guest\guest.qcow2"; Flags: ignoreversion recursesubdirs createallsubdirs

[InstallDelete]
Type: files; Name: "{autoprograms}\AI Phone.lnk"

[Icons]
Name: "{autoprograms}\AImanity"; Filename: "{app}\ai-phone-client.exe"; AppUserModelID: "dev.aiphone.client"

[Run]
Filename: "{app}\ai-phone-client.exe"; Description: "{cm:LaunchProgram,AImanity}"; Flags: nowait postinstall skipifsilent; Check: not NeedRestart

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

[CustomMessages]
english.HostSetupFailed=Device runtime setup failed.
chinese.HostSetupFailed=设备运行环境安装失败。
traditionalchinese.HostSetupFailed=裝置執行環境安裝失敗。
japanese.HostSetupFailed=デバイス実行環境のセットアップに失敗しました。
korean.HostSetupFailed=기기 실행 환경을 설치하지 못했습니다.
german.HostSetupFailed=Die Geräteumgebung konnte nicht installiert werden.
french.HostSetupFailed=Échec de l’installation de l’environnement des appareils.
spanish.HostSetupFailed=No se pudo instalar el entorno de los dispositivos.
russian.HostSetupFailed=Не удалось установить среду устройств.
arabic.HostSetupFailed=تعذر تثبيت بيئة تشغيل الأجهزة.

[Code]
var
  HostRestart: Boolean;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Base: String;
  BaseDigest: String;
  ResultCode: Integer;
begin
  Result := '';
  Base := ExpandConstant('{app}\host\qemu\guest\guest.qcow2');
  if FileExists(Base) then
  begin
    ExtractTemporaryFile('ai-phone-client.exe');
    if not ExecAsOriginalUser(ExpandConstant('{tmp}\ai-phone-client.exe'),
      '--stop-windows-backend', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) or
      (ResultCode <> 0) then
    begin
      Result := ExpandConstant('{cm:HostSetupFailed}');
      Exit;
    end;
    BaseDigest := GetSHA256OfFile(Base);
    if (CompareText(BaseDigest, '{#BaseDiskSHA256}') <> 0) and
       (CompareText(BaseDigest, '{#AluminumBaseSHA256}') <> 0) then
      Result := ExpandConstant('{cm:HostSetupFailed}');
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
  Output: TExecOutput;
  ErrorText: String;
  Index: Integer;
begin
  if (CurStep = ssPostInstall) and
    FileExists(ExpandConstant('{app}\host\qemu\manifest.json')) then
  begin
    if not ExecAndCaptureOutput(ExpandConstant('{app}\ai-phone-client.exe'),
      '--prepare-windows-host', '', SW_HIDE, ewWaitUntilTerminated, ResultCode, Output) then
      RaiseException(SysErrorMessage(ResultCode));
    if (ResultCode <> 0) and (ResultCode <> 3010) then
    begin
      ErrorText := ExpandConstant('{cm:HostSetupFailed}') + ' (' + IntToStr(ResultCode) + ')';
      for Index := 0 to GetArrayLength(Output.StdOut) - 1 do
        ErrorText := ErrorText + #13#10 + Output.StdOut[Index];
      RaiseException(ErrorText);
    end;
    HostRestart := ResultCode = 3010;
  end;
end;

function NeedRestart(): Boolean;
begin
  Result := HostRestart;
end;

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
